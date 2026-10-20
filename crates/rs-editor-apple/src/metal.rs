//! Drawing into a `CAMetalLayer` with Skia's Ganesh Metal backend.
//!
//! Swift hands over the `MTLDevice` and `MTLCommandQueue` once, and for
//! each frame the texture of the layer's next drawable; Skia draws into it
//! and submits on that queue, then Swift presents the drawable with a
//! command buffer on the same queue (so it runs after Skia's work).

use std::ffi::c_void;

use rs_editor_view::EditorView;
use skia_safe::gpu::{self, DirectContext, SurfaceOrigin, mtl};
use skia_safe::{ColorType, ImageInfo};

#[derive(Default)]
pub(crate) struct MetalSurface {
    skia: Option<DirectContext>,
    /// The menus' GPU programs were compiled (after the first frame).
    warmed: bool,
}

impl std::fmt::Debug for MetalSurface {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MetalSurface")
            .field("attached", &self.skia.is_some())
            .finish()
    }
}

impl MetalSurface {
    /// Creates the Skia context for `device` and `queue` (`MTLDevice`,
    /// `MTLCommandQueue`); Skia takes its own references.
    ///
    /// # Safety
    ///
    /// Both must be live Metal objects of that kind.
    pub(crate) unsafe fn attach(&mut self, device: *const c_void, queue: *const c_void) -> bool {
        if device.is_null() || queue.is_null() {
            return false;
        }
        // SAFETY: the caller passes a live device and queue.
        let backend = unsafe { mtl::BackendContext::new(device, queue) };
        self.skia = gpu::direct_contexts::make_metal(&backend, None);
        self.warmed = false;
        self.skia.is_some()
    }

    /// Lets go of the Skia context (the view left its window).
    pub(crate) fn detach(&mut self) {
        if let Some(mut skia) = self.skia.take() {
            skia.abandon();
        }
    }

    /// Draws a frame into `texture` (a drawable's `MTLTexture`, BGRA,
    /// `width` × `height` pixels) and submits the work.
    ///
    /// # Safety
    ///
    /// `texture` must be a live `MTLTexture` of that size.
    pub(crate) unsafe fn render(
        &mut self,
        view: &mut EditorView,
        texture: *const c_void,
        width: i32,
        height: i32,
    ) -> bool {
        let Some(skia) = self.skia.as_mut() else {
            return false;
        };
        if texture.is_null() || width <= 0 || height <= 0 {
            return false;
        }
        // SAFETY: the caller passes a live texture.
        let info = unsafe { mtl::TextureInfo::new(texture) };
        let target = gpu::backend_render_targets::make_mtl((width, height), &info);
        let Some(mut surface) = gpu::surfaces::wrap_backend_render_target(
            skia,
            &target,
            SurfaceOrigin::TopLeft,
            ColorType::BGRA8888,
            None,
            None,
        ) else {
            return false;
        };
        view.paint(surface.canvas());
        drop(surface);
        skia.flush_and_submit();
        if !self.warmed {
            // Right after the first frame: compile the menus' programs so
            // the first `/` or selection does not stall.
            self.warmed = true;
            warm_up(skia, view);
        }
        true
    }
}

/// Draws the editor's menus and popovers once offscreen (see
/// [`EditorView::paint_ui_warm_up`]).
fn warm_up(skia: &mut DirectContext, view: &EditorView) {
    let info = ImageInfo::new_n32_premul((256, 160), None);
    if let Some(mut surface) = gpu::surfaces::render_target(
        skia,
        gpu::Budgeted::Yes,
        &info,
        None,
        SurfaceOrigin::TopLeft,
        None,
        false,
        None,
    ) {
        view.paint_ui_warm_up(surface.canvas());
        skia.flush_and_submit();
    }
}
