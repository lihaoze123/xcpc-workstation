//! Flicker-free layered page view component backed by the shared [`PIXELS`] cache.
//!
//! When a page already has rendered layers and a newer set arrives, at most one
//! previous generation stays mounted underneath while the incoming layers cross-fade
//! smoothly over 90ms on the native frame clock (`transition_opacity`), then retired
//! layers are pruned once the transition settles. Rapid consecutive updates bound
//! mounted layers to `<= 2` generations, cache misses never retire valid old layers,
//! and unmount cleanup cancels pending timers cleanly.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use nsh_core::element::ImageSrc;
use nsh_core::geometry::relative;
use nsh_core::prelude::*;

use xcpc_app::PIXELS;

use crate::types::PreviewLayer;

const CROSSFADE_MS: f32 = 90.0;
const RETIRE_DELAY_MS: u64 = 110;

/// The props of [`PageImage`].
#[derive(Debug, Clone, PartialEq, Props)]
pub struct PageImageProps {
    /// A page's `layers` from `Preview.compile`.
    #[prop(default)]
    pub layers: Prop<Vec<PreviewLayer>>,
}

/// A page `Preview.compile` rasterized, its layers stretched over the box (size it as the
/// page). New layers show together once all of them have loaded; until then the layers
/// they replace stay, so the page never goes blank (nor blurred, by a new whole-page
/// layer over old tiles) while an edit or a zoom lands.
#[component]
pub fn PageImage(props: PageImageProps, _cx: Scope<NoEvent>) -> impl IntoElement {
    let ready = signal(HashSet::<String>::new());
    let shown = signal(Vec::<PreviewLayer>::new());
    let live = signal(HashSet::<String>::new());
    let animate_in = signal(false);

    // Atomic guards so unmount cleanup safely cancels any in-flight retire timer
    // without touching disposed signals.
    let retire_seq = Arc::new(AtomicU64::new(0));
    let mounted = Arc::new(AtomicBool::new(true));
    {
        let mounted = mounted.clone();
        let retire_seq = retire_seq.clone();
        cleanup(move || {
            mounted.store(false, Ordering::Release);
            retire_seq.fetch_add(1, Ordering::AcqRel);
        });
    }

    let settle = {
        let retire_seq = retire_seq.clone();
        let mounted = mounted.clone();
        move |current: &[PreviewLayer]| {
            if !mounted.load(Ordering::Acquire) || current.is_empty() {
                return;
            }
            if current
                .iter()
                .all(|l| ready.with_untracked(|r| r.contains(&l.key)))
            {
                let keep: HashSet<String> = current.iter().map(|l| l.key.clone()).collect();
                let prev_live = live.get_untracked();
                let had_live = !prev_live.is_empty();
                let has_retiring =
                    shown.with_untracked(|s| s.iter().any(|l| !keep.contains(&l.key)));

                // Cap stacked generations to at most 1 retiring frame + 1 incoming frame so
                // continuous rapid typing (<110ms) never accumulates unbounded RGBA layers.
                let capped: Vec<PreviewLayer> = shown
                    .get_untracked()
                    .into_iter()
                    .filter(|l| keep.contains(&l.key) || prev_live.contains(&l.key))
                    .collect();

                animate_in.set(had_live && has_retiring);

                let seq = retire_seq.fetch_add(1, Ordering::AcqRel).wrapping_add(1);

                if had_live && has_retiring {
                    let mut visible: HashSet<String> = prev_live;
                    visible.extend(keep.iter().cloned());
                    shown.set(capped);
                    live.set(visible);

                    let current_owned = current.to_vec();
                    let retire_seq_t = retire_seq.clone();
                    let mounted_t = mounted.clone();
                    let _ = set_timeout(Duration::from_millis(RETIRE_DELAY_MS), move || {
                        if !mounted_t.load(Ordering::Acquire)
                            || retire_seq_t.load(Ordering::Acquire) != seq
                        {
                            return;
                        }
                        shown.set(current_owned);
                        ready.update(|r| r.retain(|k| keep.contains(k)));
                        live.set(keep);
                    });
                } else {
                    shown.set(current.to_vec());
                    ready.update(|r| r.retain(|k| keep.contains(k)));
                    live.set(keep);
                }
            }
        }
    };

    {
        let settle = settle.clone();
        effect(move || {
            let current = props.layers.get();
            untracked(|| {
                if current.is_empty() {
                    return;
                }
                let prev_live = live.get_untracked();
                // Keep only currently live layers underneath the incoming generation so
                // superseded unready generations are pruned immediately.
                let mut next: Vec<PreviewLayer> = shown
                    .get_untracked()
                    .into_iter()
                    .filter(|l| {
                        prev_live.contains(&l.key) && !current.iter().any(|c| c.key == l.key)
                    })
                    .collect();
                next.extend(current.iter().cloned());
                shown.set(next);
                settle(&current);
            });
        });
    }

    view().relative().size_full().child(each(
        move || shown.get(),
        |layer: &PreviewLayer| layer.key.clone(),
        move |layer, _| {
            let layer = layer.get_untracked();
            let key = layer.key.clone();
            let cached_src = PIXELS
                .lock()
                .unwrap()
                .get(&key)
                .map(|px| ImageSrc::rgba(px.width, px.height, px.data));
            let has_pixels = cached_src.is_some();
            let src = cached_src.unwrap_or_else(|| ImageSrc::from(""));
            let arrived = {
                let settle = settle.clone();
                let mounted = mounted.clone();
                move |key: &String| {
                    if !mounted.load(Ordering::Acquire) || !has_pixels {
                        // On a PIXELS cache miss, never mark the empty placeholder ready
                        // so valid older layers stay visible instead of flashing blank white.
                        return;
                    }
                    ready.update(|r| {
                        r.insert(key.clone());
                    });
                    untracked(|| settle(&props.layers.get_untracked()));
                }
            };
            let is_live = {
                let key = key.clone();
                move || live.with(|r| r.contains(&key))
            };
            image(src)
                .absolute()
                .left(relative(layer.left as f32))
                .top(relative(layer.top as f32))
                .w(relative(layer.width as f32))
                .h(relative(layer.height as f32))
                .transition_opacity()
                .duration(move || if animate_in.get() { CROSSFADE_MS } else { 0.0 })
                .opacity(move || if is_live() { 1.0_f32 } else { 0.0 })
                .on_load({
                    let key = key.clone();
                    let arrived = arrived.clone();
                    move |_: &Size<DevicePixels>| arrived(&key)
                })
                .on_error(move |_: &str| arrived(&key))
        },
    ))
}
