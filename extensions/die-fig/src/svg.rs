//! A scene to SVG at 1×: paths as `<path>`, text as `<text>` in a system font stack with
//! `tabular-nums` where asked, gradients as `<linearGradient>`, clips as `<clipPath>`.

use crate::scene::Scene;

/// The scene as an SVG document at 1× (text as `<text>` with a system font stack and
/// tabular-nums where asked; gradients as `<linearGradient>`; clips as `<clipPath>`).
pub fn write(_scene: &Scene) -> String {
    todo!("P1-render: svg::write")
}
