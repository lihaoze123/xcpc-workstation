//! Rust-side keymap bridge for Vim `Ctrl+[` (= Escape).
//!
//! `nsh-base`'s `<Editor>` binds `mod+[` (`Outdent`, which is `Ctrl+[` on Linux)
//! as a Rust component keybinding. Because `nsh-core` appends Rust component
//! bindings after JS `keymap.bind` entries (and resolves newest-first), a JS
//! binding cannot override `Ctrl+[` while `<Editor>` is focused. Mounting this
//! zero-size Rust component with `active` driven by editor focus registers
//! `ctrl+[` *after* `<Editor>`'s focus bindings, cleanly taking precedence only
//! while Vim mode is active and the editor has focus.

use nsh_core::keymap::When;
use nsh_core::prelude::*;

/// Props for [`VimKeyBridge`].
#[derive(Debug, Clone, PartialEq, Props)]
pub struct VimKeyBridgeProps {
    /// True only while Vim mode is active and the editor is focused.
    #[prop(default)]
    pub active: Prop<bool>,
}

/// Events emitted by [`VimKeyBridge`].
#[derive(Debug, Clone, PartialEq, Event)]
pub enum VimKeyBridgeEvent {
    /// Emitted when `Ctrl+[` is pressed while active.
    Escape,
}

/// Zero-size native bridge that intercepts `Ctrl+[` ahead of `<Editor>`'s
/// built-in `mod+[` (`Outdent`) binding when Vim mode is active.
#[component]
pub fn VimKeyBridge(props: VimKeyBridgeProps, cx: Scope<VimKeyBridgeEvent>) -> impl IntoElement {
    effect(move || {
        if !props.active.get() {
            return;
        }
        on_key_with(
            "ctrl+[",
            KeyOptions {
                when: When::Always,
                repeat: false,
                for_focus: true,
                ..Default::default()
            },
            move |_| cx.emit(VimKeyBridgeEvent::Escape),
        );
    });
    view()
}
