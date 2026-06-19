// core/msx-parser/src/canvas.rs
use dixscript::Runtime::DixData;
use msx_ast::{Canvas, Color, ViewBox};

use crate::dix_helpers::{color_from, opt, path_present, raw};

pub fn parse_canvas(data: &DixData) -> Result<Canvas, String> {
    let width = opt::<f64>(data, "scene", "width")?
        .ok_or_else(|| "scene.width: missing required field".to_string())?;
    let height = opt::<f64>(data, "scene", "height")?
        .ok_or_else(|| "scene.height: missing required field".to_string())?;
    let background = color_from(raw(data, "scene", "background")).unwrap_or(Color::WHITE);

    let mut canvas = Canvas::new(width, height, background);

    if path_present(data, "viewbox") {
        let min_x = opt::<f64>(data, "viewbox", "min_x")?.unwrap_or(0.0);
        let min_y = opt::<f64>(data, "viewbox", "min_y")?.unwrap_or(0.0);
        let vb_w  = opt::<f64>(data, "viewbox", "width")?.unwrap_or(width);
        let vb_h  = opt::<f64>(data, "viewbox", "height")?.unwrap_or(height);
        canvas.viewbox = Some(ViewBox::new(min_x, min_y, vb_w, vb_h));
    }

    Ok(canvas)
                           }
