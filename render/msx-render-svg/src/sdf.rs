// render/msx-render-svg/src/sdf.rs
use msx_ast::{SdfNode, SdfTree};

use crate::Ctx;

/// SDF compound shapes have no SVG 1.1 equivalent — same call
/// `Def::ConicGradient::to_svg()` already makes for conic gradients.
/// `msx-render-cpu` / `msx-render-gpu` evaluate the same `SdfTree` math
/// directly (per-pixel / per-fragment) and are the source of truth.
pub(crate) fn render_sdf(ctx: &mut Ctx, node: &SdfNode) {
    let label = tree_label(&node.tree);
    ctx.push(&format!(
        "<!-- sdf node '{}': use msx-render-cpu/msx-render-gpu for native support -->",
        label
    ));
}

fn tree_label(tree: &SdfTree) -> &'static str {
    match tree {
        SdfTree::Circle { .. } => "circle",
        SdfTree::Box { .. } => "box",
        SdfTree::Line { .. } => "line",
        SdfTree::Ring { .. } => "ring",
        SdfTree::Arc { .. } => "arc",
        SdfTree::Union(_) => "union",
        SdfTree::SmoothUnion { .. } => "smooth_union",
        SdfTree::Subtract { .. } => "subtract",
        SdfTree::SmoothSubtract { .. } => "smooth_subtract",
        SdfTree::Intersect { .. } => "intersect",
        SdfTree::SmoothIntersect { .. } => "smooth_intersect",
        SdfTree::Offset { .. } => "offset",
    }
      }
