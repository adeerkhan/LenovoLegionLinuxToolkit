//! Offscreen rendering, for `legiontoolkit --shot <path>`.
//!
//! This exists because visual correctness is otherwise unverifiable: without a
//! display, the only way to know a page lays out correctly is to look at it, and
//! the usual screenshot tools do not work under Wayland. `gsk::CairoRenderer`
//! renders the widget tree to a texture with no display at all.
//!
//! It is also the honest way to review this application's own layout, and it
//! gives the README something real to show.

use gtk4::prelude::*;
use gtk4::{gdk, gsk, Snapshot, WidgetPaintable};

/// Render a mapped widget to a PNG.
pub fn capture(window: &gtk4::Window, path: &str) -> anyhow::Result<()> {
    let widget = window.upcast_ref::<gtk4::Widget>();
    let width = widget.width();
    let height = widget.height();

    if width <= 0 || height <= 0 {
        anyhow::bail!("window has no allocation ({width}x{height}); not mapped yet?");
    }

    let paintable = WidgetPaintable::new(Some(widget));
    let snapshot = Snapshot::new();
    paintable.snapshot(&snapshot, width as f64, height as f64);

    let node = snapshot
        .to_node()
        .ok_or_else(|| anyhow::anyhow!("no render node produced"))?;

    let renderer = gsk::CairoRenderer::new();
    renderer.realize(None::<&gdk::Surface>)?;
    let texture = renderer.render_texture(&node, None);
    renderer.unrealize();

    texture
        .save_to_png(path)
        .map_err(|e| anyhow::anyhow!("cannot write {path}: {e}"))?;

    tracing::info!("wrote {path} ({width}x{height})");
    Ok(())
}
