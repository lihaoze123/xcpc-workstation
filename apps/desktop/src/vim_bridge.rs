//! Rust-side keymap bridge for Vim `Ctrl+[` and `Escape`.
//!
//! `nsh-base`'s `<Editor>` binds `mod+[` (`Outdent`, which is `Ctrl+[` on Linux)
//! while focused, and binds `escape` (`SingleSelection`) whenever a non-empty
//! selection exists (such as Visual mode). Because `nsh-core` appends Rust
//! component bindings after JS `keymap.bind` entries (and resolves newest-first),
//! JS bindings cannot override them. Mounting this zero-size Rust component with
//! `active` and `mode` registers both keys *after* `<Editor>`'s bindings whenever
//! focus or Vim mode changes, cleanly giving Vim single-keypress `Esc` / `Ctrl+[`.

use nsh_core::keymap::When;
use nsh_core::prelude::*;

/// Props for [`VimKeyBridge`].
#[derive(Debug, Clone, PartialEq, Props)]
pub struct VimKeyBridgeProps {
    /// True only while Vim mode is active and the editor is focused.
    #[prop(default)]
    pub active: Prop<bool>,
    /// Current Vim mode name (e.g. `"normal"`, `"insert"`, `"visual"`, `"visual-line"`).
    /// Changing `mode` re-registers the Rust bindings after `<Editor>`'s `collapsible`
    /// selection binding so `Escape` in Visual mode exits Visual in one keypress.
    #[prop(default)]
    pub mode: Prop<String>,
}

/// Events emitted by [`VimKeyBridge`].
#[derive(Debug, Clone, PartialEq, Event)]
pub enum VimKeyBridgeEvent {
    /// Emitted when `Escape` is pressed while active.
    Escape,
    /// Emitted when `Ctrl+[` is pressed while active.
    CtrlBracket,
}

/// Zero-size native bridge that intercepts `Escape` and `Ctrl+[` ahead of
/// `<Editor>`'s built-in Rust bindings when Vim mode is active.
#[component]
pub fn VimKeyBridge(props: VimKeyBridgeProps, cx: Scope<VimKeyBridgeEvent>) -> impl IntoElement {
    effect(move || {
        let _mode = props.mode.get();
        if !props.active.get() {
            return;
        }
        let opts = KeyOptions {
            when: When::Always,
            repeat: false,
            for_focus: true,
            ..Default::default()
        };
        on_key_with("escape", opts, move |_| cx.emit(VimKeyBridgeEvent::Escape));
        on_key_with("ctrl+[", opts, move |_| {
            cx.emit(VimKeyBridgeEvent::CtrlBracket)
        });
    });
    view()
}
