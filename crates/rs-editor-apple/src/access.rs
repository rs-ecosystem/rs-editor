//! VoiceOver: AccessKit's adapter for the view (`accesskit_macos` on
//! macOS, `accesskit_ios` on iOS), fed the tree the core builds, the same
//! one TalkBack gets on Android.
//!
//! The adapter subclasses the Swift view at run time to answer the
//! accessibility protocol. Actions from VoiceOver (move the caret, press a
//! menu button) edit through the core; the view is then told with
//! `rsEditorAccessibilityChanged` so it redraws and resyncs text input.

use std::ffi::c_void;

use accesskit::{ActionHandler, ActionRequest, ActivationHandler, TreeId, TreeUpdate};
use objc2::msg_send;
use objc2::runtime::AnyObject;

use crate::api::NativeEditor;

#[cfg(target_os = "ios")]
type Adapter = accesskit_ios::SubclassingAdapter;
#[cfg(target_os = "macos")]
type Adapter = accesskit_macos::SubclassingAdapter;

pub(crate) struct Accessibility {
    adapter: Adapter,
}

impl std::fmt::Debug for Accessibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Accessibility").finish_non_exhaustive()
    }
}

/// Answers the adapter on the main thread, where Swift calls in too.
struct Handler {
    editor: *mut NativeEditor,
    view: *mut c_void,
}

impl Handler {
    fn editor(&mut self) -> &mut NativeEditor {
        // SAFETY: BoltFFI keeps the editor boxed (a stable address) until
        // Swift releases it, which drops this adapter first; all calls are
        // on the main thread.
        unsafe { &mut *self.editor }
    }
}

impl ActivationHandler for Handler {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        Some(self.editor().api.view_mut().accessibility_tree())
    }
}

impl ActionHandler for Handler {
    fn do_action(&mut self, request: ActionRequest) {
        if !self.editor().api.view_mut().accessibility_action(&request) {
            return;
        }
        // SAFETY: the view outlives its adapter, and implements the method.
        unsafe {
            let view = &*(self.view as *const AnyObject);
            let _: () = msg_send![view, rsEditorAccessibilityChanged];
        }
    }
}

#[cfg(target_os = "ios")]
impl accesskit::DeactivationHandler for Handler {
    fn deactivate_accessibility(&mut self) {}
}

impl Accessibility {
    /// Makes `view` (the editor's `NSView` or `UIView`) answer VoiceOver.
    ///
    /// # Safety
    ///
    /// `view` must be a live view that stays alive while this does, and
    /// `editor` the boxed editor that owns this.
    pub(crate) unsafe fn attach(view: *mut c_void, editor: *mut NativeEditor) -> Self {
        let handler = || Handler { editor, view };
        // SAFETY: as the caller promises.
        #[cfg(target_os = "macos")]
        let adapter = unsafe { Adapter::new(view, handler(), handler()) };
        #[cfg(target_os = "ios")]
        let adapter = unsafe { Adapter::new(view, handler(), handler(), handler()) };
        Self { adapter }
    }

    /// Sends what changed since the last frame, while VoiceOver listens.
    pub(crate) fn update(&mut self, view: &mut rs_editor_view::EditorView) {
        let events = self.adapter.update_if_active(|| {
            view.accessibility_update().unwrap_or(TreeUpdate {
                nodes: Vec::new(),
                tree: None,
                tree_id: TreeId::ROOT,
                focus: rs_editor_view::EDITOR_NODE,
            })
        });
        if let Some(events) = events {
            events.raise();
        }
    }

    /// The window became key or stopped being key (macOS).
    pub(crate) fn set_window_focused(&mut self, focused: bool) {
        #[cfg(target_os = "macos")]
        if let Some(events) = self.adapter.update_view_focus_state(focused) {
            events.raise();
        }
        #[cfg(target_os = "ios")]
        let _ = focused;
    }
}
