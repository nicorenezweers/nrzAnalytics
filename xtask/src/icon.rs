//! `icon`: zeichnet `umbrel/icon.png` (256x256, quadratisch, ohne runde Ecken – umbrelOS rundet
//! selbst). Motiv wie `web/assets/favicon.svg`: Balken in Grautoenen und eine steigende Linie.

use tiny_skia::{
    Color, FillRule, GradientStop, LineCap, LineJoin, LinearGradient, Paint, PathBuilder, Pixmap, Point,
    Rect, SpreadMode, Stroke, Transform,
};

use crate::util::{Result, root};

const SIZE: u32 = 256;

fn rgb(hex: u32) -> Color {
    Color::from_rgba8((hex >> 16) as u8, (hex >> 8) as u8, hex as u8, 255)
}

fn paint(color: Color) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(color);
    p.anti_alias = true;
    p
}

/// Rechteck mit runden Ecken (fuer die Balken).
fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.quad_to(x + w, y, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.quad_to(x + w, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.quad_to(x, y + h, x, y + h - r);
    pb.line_to(x, y + r);
    pb.quad_to(x, y, x + r, y);
    pb.close();
    pb.finish()
}

pub fn icon() -> Result<()> {
    let mut pm = Pixmap::new(SIZE, SIZE).ok_or("Pixmap anlegen")?;
    let full = Rect::from_xywh(0.0, 0.0, SIZE as f32, SIZE as f32).ok_or("Rechteck")?;

    // Dunkler Grund mit dezentem Verlauf (oben links heller).
    let shader = LinearGradient::new(
        Point::from_xy(0.0, 0.0),
        Point::from_xy(SIZE as f32, SIZE as f32),
        vec![
            GradientStop::new(0.0, rgb(0x2a2a2a)),
            GradientStop::new(1.0, rgb(0x1a1a1a)),
        ],
        SpreadMode::Pad,
        Transform::identity(),
    )
    .ok_or("Verlauf")?;
    let bg = Paint {
        shader,
        anti_alias: true,
        ..Paint::default()
    };
    pm.fill_rect(full, &bg, Transform::identity(), None);

    // Grundlinie und vier Balken in Grautoenen.
    let base = 204.0;
    if let Some(r) = Rect::from_xywh(40.0, base + 6.0, 176.0, 4.0) {
        pm.fill_rect(r, &paint(rgb(0x363636)), Transform::identity(), None);
    }
    let bars: [(f32, f32, u32); 4] = [
        (46.0, 56.0, 0x3b3b3b),
        (92.0, 96.0, 0x474747),
        (138.0, 76.0, 0x535353),
        (184.0, 132.0, 0x626262),
    ];
    for (x, h, color) in bars {
        let path = rounded_rect(x, base - h, 30.0, h, 6.0).ok_or("Balken")?;
        pm.fill_path(
            &path,
            &paint(rgb(color)),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }

    // Steigende Linie in Hellblau mit Punkt am Ende.
    let points = [(40.0, 150.0), (100.0, 98.0), (146.0, 126.0), (206.0, 52.0)];
    let mut pb = PathBuilder::new();
    pb.move_to(points[0].0, points[0].1);
    for (x, y) in &points[1..] {
        pb.line_to(*x, *y);
    }
    let line = pb.finish().ok_or("Linie")?;
    let outline = Stroke {
        width: 22.0,
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Stroke::default()
    };
    pm.stroke_path(
        &line,
        &paint(Color::from_rgba8(0x1c, 0x1c, 0x1c, 200)),
        &outline,
        Transform::identity(),
        None,
    );
    let stroke = Stroke {
        width: 13.0,
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Stroke::default()
    };
    pm.stroke_path(&line, &paint(rgb(0x9ec5ff)), &stroke, Transform::identity(), None);

    let (ex, ey) = points[3];
    let ring = PathBuilder::from_circle(ex, ey, 17.0).ok_or("Kreis")?;
    pm.fill_path(
        &ring,
        &paint(rgb(0x1f1f1f)),
        FillRule::Winding,
        Transform::identity(),
        None,
    );
    let dot = PathBuilder::from_circle(ex, ey, 11.0).ok_or("Kreis")?;
    pm.fill_path(
        &dot,
        &paint(rgb(0xffffff)),
        FillRule::Winding,
        Transform::identity(),
        None,
    );

    let path = root().join("umbrel/icon.png");
    std::fs::create_dir_all(path.parent().ok_or("Pfad")?)?;
    pm.save_png(&path)
        .map_err(|e| format!("{} schreiben: {e}", path.display()))?;
    println!("==> {} ({SIZE}x{SIZE})", path.display());
    Ok(())
}
