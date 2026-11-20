//! Flicker-free layered page view component backed by the shared [`PIXELS`] cache.

use std::collections::HashSet;

use nsh_core::element::ImageSrc;
use nsh_core::geometry::relative;
use nsh_core::prelude::*;

use crate::cache::PIXELS;
use crate::types::PreviewLayer;

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
    // The keys whose image has loaded; the layers laid out (the current ones over the
    // ones they replace); and the keys of those drawn.
    let ready = signal(HashSet::<String>::new());
    let shown = signal(Vec::<PreviewLayer>::new());
    let live = signal(HashSet::<String>::new());
    let settle = move |current: &[PreviewLayer]| {
        if current
            .iter()
            .all(|l| ready.with_untracked(|r| r.contains(&l.key)))
        {
            shown.set(current.to_vec());
            let keep: HashSet<String> = current.iter().map(|l| l.key.clone()).collect();
            ready.update(|r| r.retain(|k| keep.contains(k)));
            live.set(keep);
        }
    };
    effect(move || {
        let current = props.layers.get();
        untracked(|| {
            let mut next: Vec<PreviewLayer> = shown
                .get_untracked()
                .into_iter()
                .filter(|l| !current.iter().any(|c| c.key == l.key))
                .collect();
            next.extend(current.iter().cloned());
            shown.set(next);
            settle(&current);
        });
    });
    view().relative().size_full().child(each(
        move || shown.get(),
        |layer: &PreviewLayer| layer.key.clone(),
        move |layer, _| {
            let layer = layer.get_untracked();
            let key = layer.key.clone();
            let src = PIXELS
                .lock()
                .unwrap()
                .get(&key)
                .unwrap_or_else(|| ImageSrc::from(""));
            let arrived = move |key: &String| {
                ready.update(|r| {
                    r.insert(key.clone());
                });
                untracked(|| settle(&props.layers.get_untracked()));
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
                .opacity(move || if is_live() { 1.0_f32 } else { 0.0 })
                .on_load({
                    let key = key.clone();
                    move |_: &Size<DevicePixels>| arrived(&key)
                })
                // One that fails holds nothing up.
                .on_error(move |_: &str| arrived(&key))
        },
    ))
}
