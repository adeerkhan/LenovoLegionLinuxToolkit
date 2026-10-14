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

/// Log the visible page's structure.
///
/// A capture cannot show content below the fold — an offscreen paint does not
/// render widgets outside their scroller's visible area, whatever size the
/// paintable is given — so structural checks are done on the tree.
pub fn log_visible_page(stack: &gtk4::Stack) {
    fn walk(widget: &gtk4::Widget, depth: usize, out: &mut Vec<String>) {
        let mut child = widget.first_child();
        while let Some(current) = child {
            let current = current.upcast::<gtk4::Widget>();
            let text = match current.downcast_ref::<gtk4::Label>() {
                Some(label) => format!(" \"{}\"", label.label()),
                None => String::new(),
            };
            out.push(format!(
                "{}{}{}",
                "  ".repeat(depth),
                current.type_().name(),
                text
            ));
            walk(&current, depth + 1, out);
            child = current.next_sibling();
        }
    }
    if let Some(page) = stack.visible_child() {
        let mut out = Vec::new();
        walk(&page.upcast::<gtk4::Widget>(), 0, &mut out);
        tracing::warn!("page tree ({} nodes):\n{}", out.len(), out.join("\n"));
    }
}

/// Render a mapped widget to a PNG.
pub fn capture(window: &gtk4::Window, path: &str) -> anyhow::Result<()> {
    let widget = window.upcast_ref::<gtk4::Widget>();
    let width = widget.width();
    let height = widget.height();

    if width <= 0 || height <= 0 {
        anyhow::bail!("window has no allocation ({width}x{height}); not mapped yet?");
    }

    let paintable = WidgetPaintable::new(Some(widget));
    render(&paintable, width, height, path)?;
    tracing::info!("wrote {path} ({width}x{height})");
    Ok(())
}

/// Paint a `gdk::Paintable` into a PNG at the given size.
fn render(
    paintable: &impl gtk4::prelude::IsA<gtk4::gdk::Paintable>,
    width: i32,
    height: i32,
    path: &str,
) -> anyhow::Result<()> {
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
    Ok(())
}
