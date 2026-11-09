//! Operator panel widths and migration from the previous defaults.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OperatorPanelSide {
    Left,
    Right,
}

pub const OPERATOR_PANEL_DEFAULTS: (f64, f64) = (280.0, 460.0);
pub const OPERATOR_PANEL_BOUNDS: ((f64, f64), (f64, f64)) = ((180.0, 420.0), (420.0, 680.0));

pub fn resize_operator_panel(side: OperatorPanelSide, start: f64, delta: f64) -> f64 {
    let (requested, (min, max)) = match side {
        OperatorPanelSide::Left => (start + delta, OPERATOR_PANEL_BOUNDS.0),
        OperatorPanelSide::Right => (start - delta, OPERATOR_PANEL_BOUNDS.1),
    };
    crate::js::clamp(crate::js::math_round(requested), min, max)
}

pub fn normalize_operator_panel_width(
    side: OperatorPanelSide,
    stored: Option<&str>,
    fallback: Option<f64>,
) -> f64 {
    let (default, legacy) = match side {
        OperatorPanelSide::Left => (OPERATOR_PANEL_DEFAULTS.0, 240.0),
        OperatorPanelSide::Right => (OPERATOR_PANEL_DEFAULTS.1, 320.0),
    };
    let width = stored
        .map(crate::js::string_to_number)
        .filter(|width| width.is_finite())
        .map_or(fallback.unwrap_or(default), |width| {
            if width == legacy { default } else { width }
        });
    resize_operator_panel(side, width, 0.0)
}
