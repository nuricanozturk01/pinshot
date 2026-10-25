//! One capture session shared by the per-monitor overlay windows: the screenshots, the editor state, the toolbar,
//! and the pure function that draws a window's frame. Nothing here touches the window system, so frames are tested
//! by comparing pixels.

use crate::gfx::IRect;
use crate::recpanel::RecPanel;
use crate::screens::Capture;
use crate::toolbar::{self, Toolbar};
use crate::ui::{theme, Canvas};
use pinshot_core::annotate::{self, Kind};
use pinshot_core::compose::compose_region;
use pinshot_core::editor::{Editor, TextEdit, Tool};
use pinshot_core::render::{self, View};
use pinshot_core::text::Fonts;
use pinshot_core::{Point, Rect, Rgba};
use tiny_skia::{Pixmap, PixmapPaint, Transform};

/// Opacity of the dimming layer over everything outside the selection.
const DIM_ALPHA: u32 = 120;
const LOUPE_N: i32 = 15;
const LOUPE_Z: i32 = 8;

/// How a window relates to its monitor.
#[derive(Clone, Copy, Debug)]
pub struct Geom {
    pub mon: usize,
    /// UI scale (physical pixels per logical UI unit).
    pub sf: f32,
    pub size: (u32, u32),
}

pub struct Session {
    pub caps: Vec<Capture>,
    pub editor: Editor,
    pub toolbar: Toolbar,
    /// Last known pointer position in desktop coordinates.
    pub cursor: Option<Point>,
    pub tooltip: Option<(String, Rect)>,
    /// Set while a modal popup (record options, colour picker) is open above the toolbar.
    /// Recording options (replaces the toolbar while recording is being set up).
    pub rec: Option<RecPanel>,
    /// The overlay was opened by the record command (cancelling closes it).
    pub rec_flow: bool,
}

pub fn desktop_bounds(caps: &[Capture]) -> Rect {
    caps.iter()
        .map(|c| c.monitor.rect)
        .reduce(|a, b| a.union(&b))
        .unwrap_or_default()
}

impl Session {
    pub fn new(caps: Vec<Capture>, color: Rgba, pen_w: u32, font_px: u32) -> Self {
        let bounds = desktop_bounds(&caps);
        let mut s = Session {
            caps,
            editor: Editor::new(bounds, color, pen_w, font_px),
            toolbar: Toolbar::default(),
            cursor: None,
            tooltip: None,
            rec: None,
            rec_flow: false,
        };
        s.sync_toolbar();
        s
    }

    /// Mirror the editor's state into the toolbar (call after any editor change).
    pub fn sync_toolbar(&mut self) {
        let e = &self.editor;
        let t = &mut self.toolbar;
        let kind = e.property_kind();
        let changed = t.prop_kind != kind;
        t.prop_kind = kind;
        t.tool = e.tool;
        match e.selected_item().filter(|_| e.tool == Tool::Select) {
            Some(it) => {
                t.color = it.color;
                t.width = it.width;
                t.fill = it.fill;
                t.dashed = it.dashed;
                t.font_px = it.font_px;
            }
            None => {
                t.color = e.color;
                t.width = e.pen_w;
                t.fill = e.fill;
                t.dashed = e.dashed;
                t.font_px = e.font_px;
            }
        }
        t.can_undo = e.can_undo();
        t.can_redo = e.can_redo();
        t.can_delete = e.selected.is_some();
        if changed {
            t.relayout();
        }
    }

    // ------------------------------------------------------------------ geometry
    /// Desktop point → pixel in the window of monitor `mon`.
    pub fn to_px(&self, mon: usize, p: Point) -> Point {
        let m = &self.caps[mon].monitor;
        Point::new((p.x - m.rect.x) * m.scale, (p.y - m.rect.y) * m.scale)
    }

    /// Window pixel → desktop point.
    #[expect(
        clippy::wrong_self_convention,
        reason = "reads as the inverse of to_px"
    )]
    pub fn from_px(&self, mon: usize, p: Point) -> Point {
        let m = &self.caps[mon].monitor;
        Point::new(m.rect.x + p.x / m.scale, m.rect.y + p.y / m.scale)
    }

    pub fn monitor_at(&self, p: Point) -> Option<usize> {
        crate::screens::monitor_at(
            &self
                .caps
                .iter()
                .map(|c| c.monitor.clone())
                .collect::<Vec<_>>(),
            p,
        )
    }

    /// The monitor that shows the toolbar: the one holding the selection's bottom-right corner.
    pub fn host(&self) -> Option<usize> {
        let s = self.editor.sel?;
        let corner = Point::new(s.right() - 1.0, s.bottom() - 1.0);
        if let Some(i) = self
            .caps
            .iter()
            .position(|c| c.monitor.rect.contains(corner))
        {
            return Some(i);
        }
        (0..self.caps.len()).max_by(|&a, &b| {
            let area = |i: usize| {
                let r = self.caps[i].monitor.rect.intersection(&s);
                r.w * r.h
            };
            area(a)
                .partial_cmp(&area(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    }

    /// Selection part on monitor `mon`, in that window's pixels.
    fn sel_px(&self, mon: usize) -> Option<Rect> {
        let sel = self.editor.sel?;
        let part = self.caps[mon].monitor.rect.intersection(&sel);
        if part.is_empty() {
            return None;
        }
        let a = self.to_px(mon, part.top_left());
        let b = self.to_px(mon, Point::new(part.right(), part.bottom()));
        Some(Rect::from_points(a, b))
    }

    /// Toolbar top-left in the host window's UI units, if the toolbar is visible there.
    pub fn toolbar_origin(&self, g: &Geom) -> Option<Point> {
        if self.rec.is_some()
            || self.editor.sel.is_none()
            || self.editor.is_selecting()
            || self.host() != Some(g.mon)
            || self.popup_open_hides_bar()
        {
            return None;
        }
        let vis = self.sel_px(g.mon)?;
        let ui_sel = Rect::new(vis.x / g.sf, vis.y / g.sf, vis.w / g.sf, vis.h / g.sf);
        let area = Rect::new(0.0, 0.0, g.size.0 as f32 / g.sf, g.size.1 as f32 / g.sf);
        Some(toolbar::place(ui_sel, self.toolbar.size(), area))
    }

    /// Top-left of the recording panel in the window's UI units, if it is shown there.
    pub fn panel_origin(&self, g: &Geom) -> Option<Point> {
        let panel = self.rec.as_ref()?;
        let (pw, ph) = panel.size();
        let area = Rect::new(0.0, 0.0, g.size.0 as f32 / g.sf, g.size.1 as f32 / g.sf);
        if self.editor.sel.is_some() {
            if self.editor.is_selecting() || self.host() != Some(g.mon) {
                return None;
            }
            let vis = self.sel_px(g.mon)?;
            let ui_sel = Rect::new(vis.x / g.sf, vis.y / g.sf, vis.w / g.sf, vis.h / g.sf);
            return Some(toolbar::place(ui_sel, (pw, ph), area));
        }
        // no selection yet: top centre of the monitor under the pointer
        let at = self.cursor.and_then(|c| self.monitor_at(c)).unwrap_or(0);
        (at == g.mon).then(|| Point::new((area.w - pw) / 2.0, 70.0))
    }

    fn popup_open_hides_bar(&self) -> bool {
        false
    }

    // ------------------------------------------------------------------ output
    /// The final picture: the selected region with all annotations. Returns the image and its pixels per unit.
    pub fn render_selection(&self) -> Option<(Pixmap, f32)> {
        let sel = self.editor.sel?;
        let screens: Vec<_> = self.caps.iter().map(|c| c.screen()).collect();
        let (mut out, scale) = compose_region(&screens, sel)?;
        let items = self.editor.visible_items();
        render::render_items(&mut out, &items, &View::new(sel.top_left(), scale));
        Some((out, scale))
    }

    /// Colour of the screenshot pixel under desktop point `p`.
    pub fn pixel_at(&self, p: Point) -> Option<Rgba> {
        let i = self.monitor_at(p)?;
        let q = self.to_px(i, p);
        let px = self.caps[i]
            .pixmap
            .pixel(q.x.max(0.0) as u32, q.y.max(0.0) as u32)?;
        Some(Rgba::rgb(px.red(), px.green(), px.blue()))
    }

    // ------------------------------------------------------------------ frame
    /// Everything drawn on top of the dimmed screenshot in this window, as a pixel rectangle.
    /// The strip at the top of the window that holds the hint pill (drawn while nothing is selected).
    pub fn hint_strip(&self, g: &Geom) -> IRect {
        IRect::new(0, 0, g.size.0, (60.0 * g.sf) as u32)
    }

    pub fn extent(&self, g: &Geom) -> Option<IRect> {
        let mut ext: Option<IRect> = None;
        let mut add = |r: IRect| ext = Some(ext.map_or(r, |e| e.union(&r)));
        let full = IRect::new(0, 0, g.size.0, g.size.1);
        if let Some(s) = self.sel_px(g.mon) {
            add(IRect::around(s.x, s.y, s.w, s.h).inflate((44.0 * g.sf) as i32));
        }
        if let (Some(o), Some(p)) = (self.panel_origin(g), self.rec.as_ref()) {
            let (w, h) = p.size();
            add(IRect::around(o.x * g.sf, o.y * g.sf, w * g.sf, h * g.sf).inflate(4));
        }
        if let Some(o) = self.toolbar_origin(g) {
            let (w, h) = self.toolbar.size();
            add(IRect::around(o.x * g.sf, o.y * g.sf, w * g.sf, h * g.sf + 40.0 * g.sf).inflate(4));
        }
        if self.editor.sel.is_none() || self.editor.mode == pinshot_core::editor::Mode::SelNew {
            if let Some(c) = self
                .cursor
                .filter(|c| self.caps[g.mon].monitor.rect.contains(*c))
            {
                let p = self.to_px(g.mon, c);
                // the loupe sits 22 px from the pointer, on any side, with two text pills under it that are
                // wider than the box; be generous so no part of it is left behind when the pointer moves
                let box_px = LOUPE_N * LOUPE_Z;
                let left = ((box_px + 40) as f32 * g.sf) as i32;
                let right = (360.0 * g.sf) as i32;
                let up = ((box_px + 70) as f32 * g.sf) as i32;
                let down = ((box_px + 100) as f32 * g.sf) as i32;
                add(IRect::new(
                    p.x as i32 - left,
                    p.y as i32 - up,
                    (left + right) as u32,
                    (up + down) as u32,
                ));
            }
        }
        ext.and_then(|e| e.intersect(&full))
    }

    /// Draw the part `damage` of the window's frame.
    pub fn compose(&self, g: &Geom, damage: IRect) -> Pixmap {
        let cap = &self.caps[g.mon];
        let msc = cap.monitor.scale;
        let mut out = Pixmap::new(damage.w.max(1), damage.h.max(1)).expect("frame size");
        dim_copy(&cap.pixmap, damage, &mut out);

        let sf = g.sf;
        let mut cv = Canvas::with_offset(&mut out, sf, (damage.x as f32, damage.y as f32));

        if let Some(sel) = self.editor.sel {
            if let Some(vis) = self.sel_px(g.mon) {
                self.draw_selection_content(&mut cv, cap, vis, damage);
                self.draw_selection_chrome(&mut cv, g, sel, vis, msc);
            }
        } else if self.editor.mode == pinshot_core::editor::Mode::Idle {
            let txt = pinshot_core::i18n::tr(
                "Alanı seçmek için sürükle  ·  Ctrl+A tüm ekran  ·  C renk kopyala  ·  Esc iptal",
            );
            let w = cv.text_width(txt, 12.0, false) + 16.0;
            cv.pill((g.size.0 as f32 / sf - w) / 2.0, 24.0, txt);
        }
        if self.editor.sel.is_none() || self.editor.mode == pinshot_core::editor::Mode::SelNew {
            self.draw_loupe(&mut cv, g);
        }
        if let (Some(o), Some(p)) = (self.panel_origin(g), self.rec.as_ref()) {
            p.draw(&mut cv, o);
        }
        if let Some(o) = self.toolbar_origin(g) {
            self.toolbar.draw(&mut cv, o);
            if let Some((text, r)) = &self.tooltip {
                let w = cv.text_width(text, 12.0, false) + 16.0;
                let x = (o.x + r.x + r.w / 2.0 - w / 2.0).max(4.0);
                let below = o.y + self.toolbar.size().1 + 6.0;
                let y = if below + 26.0 < g.size.1 as f32 / sf {
                    below
                } else {
                    o.y - 28.0
                };
                cv.pill(x, y, text);
            }
        }
        out
    }

    fn draw_selection_content(&self, cv: &mut Canvas, cap: &Capture, vis: Rect, damage: IRect) {
        let region = IRect::around(vis.x, vis.y, vis.w, vis.h);
        let Some(mut sub) = crop(&cap.pixmap, region) else {
            return;
        };
        let msc = cap.monitor.scale;
        let origin = Point::new(
            cap.monitor.rect.x + region.x as f32 / msc,
            cap.monitor.rect.y + region.y as f32 / msc,
        );
        let view = View::new(origin, msc);
        render::render_items(&mut sub, &self.editor.visible_items(), &view);
        if let Some(ed) = &self.editor.edit {
            let (color, px) = self.editor.edit_look();
            draw_text_edit(&mut sub, ed, color, px, &view);
        }
        let paint = PixmapPaint {
            blend_mode: tiny_skia::BlendMode::Source,
            ..PixmapPaint::default()
        };
        cv.pm.draw_pixmap(
            region.x - damage.x,
            region.y - damage.y,
            sub.as_ref(),
            &paint,
            Transform::identity(),
            None,
        );
    }

    fn draw_selection_chrome(&self, cv: &mut Canvas, g: &Geom, sel: Rect, vis: Rect, msc: f32) {
        let sf = g.sf;
        let ui = |p: Point| {
            let q = self.to_px(g.mon, p);
            Point::new(q.x / sf, q.y / sf)
        };
        let ui_vis = Rect::new(vis.x / sf, vis.y / sf, vis.w / sf, vis.h / sf);
        // selected item outline + handles
        if let (Some(it), Tool::Select) = (self.editor.selected_item(), self.editor.tool) {
            if self.editor.edit.as_ref().and_then(|e| e.item) != Some(it.id) {
                let b = annotate::Item::bounds(it, Fonts::embedded());
                let (a, c) = (ui(b.top_left()), ui(Point::new(b.right(), b.bottom())));
                cv.stroke_rect(Rect::from_points(a, c), 1.0, theme::ACCENT, true);
                for (pos, _) in annotate::handles(it) {
                    let p = ui(pos);
                    cv.fill_circle(p.x, p.y, 5.0, theme::ACCENT);
                    cv.fill_circle(p.x, p.y, 3.8, Rgba::WHITE);
                }
            }
        }
        // selection border
        cv.stroke_rect(ui_vis, 1.0, theme::ACCENT, false);
        for (_, h) in self.editor.sel_handles() {
            let tl = ui(h.top_left());
            let r = Rect::new(tl.x, tl.y, 8.0, 8.0);
            if r.intersects(&ui_vis.inflated(8.0)) {
                cv.fill_rect(r, theme::ACCENT);
                cv.fill_rect(r.inflated(-1.0), Rgba::WHITE);
            }
        }
        // size label on the monitor that holds the top-left corner
        if self.caps[g.mon].monitor.rect.contains(sel.top_left()) {
            let scale = self
                .caps
                .iter()
                .filter(|c| c.monitor.rect.intersects(&sel))
                .map(|c| c.monitor.scale)
                .fold(msc, f32::max);
            let txt = format!("{} × {}", (sel.w * scale).round(), (sel.h * scale).round());
            let tl = ui(sel.top_left());
            let y = if tl.y > 32.0 { tl.y - 28.0 } else { tl.y + 6.0 };
            cv.pill(tl.x.max(4.0), y, &txt);
        }
    }

    fn draw_loupe(&self, cv: &mut Canvas, g: &Geom) {
        let Some(cur) = self
            .cursor
            .filter(|c| self.caps[g.mon].monitor.rect.contains(*c))
        else {
            return;
        };
        let cap = &self.caps[g.mon];
        let sf = g.sf;
        let p = self.to_px(g.mon, cur);
        let (cx, cy) = (p.x as i32, p.y as i32);
        let z = ((LOUPE_Z as f32 * sf).round() as i32).max(2);
        let size = LOUPE_N * z;
        let (mut bx, mut by) = (cx + (22.0 * sf) as i32, cy + (22.0 * sf) as i32);
        if bx + size + 4 > g.size.0 as i32 {
            bx = cx - (22.0 * sf) as i32 - size;
        }
        if by + size + (56.0 * sf) as i32 > g.size.1 as i32 {
            by = cy - (58.0 * sf) as i32 - size;
        }
        // zoomed pixels, written directly (no anti-aliased seams between cells)
        let (ox, oy) = (cv.off.0 as i32, cv.off.1 as i32);
        let (pw, ph) = (cv.pm.width() as i32, cv.pm.height() as i32);
        let src = &cap.pixmap;
        let data = cv.pm.data_mut();
        for j in 0..LOUPE_N {
            for i in 0..LOUPE_N {
                let (sx, sy) = (cx - LOUPE_N / 2 + i, cy - LOUPE_N / 2 + j);
                let c = if sx >= 0
                    && sy >= 0
                    && (sx as u32) < src.width()
                    && (sy as u32) < src.height()
                {
                    let q = src.pixel(sx as u32, sy as u32).expect("in range");
                    [q.red(), q.green(), q.blue()]
                } else {
                    [0, 0, 0]
                };
                let x_from = (bx + i * z - ox).max(0);
                let x_to = (bx + i * z + z - ox).min(pw);
                if x_to <= x_from {
                    continue;
                }
                let cell = [c[0], c[1], c[2], 255];
                for yy in 0..z {
                    let y = by + j * z + yy - oy;
                    if y < 0 || y >= ph {
                        continue;
                    }
                    let row = &mut data[((y * pw + x_from) * 4) as usize..]
                        [..((x_to - x_from) * 4) as usize];
                    for px in row.as_chunks_mut::<4>().0 {
                        *px = cell;
                    }
                }
            }
        }
        let bx_ui = bx as f32 / sf;
        let by_ui = by as f32 / sf;
        let sz_ui = size as f32 / sf;
        let boxr = Rect::new(bx_ui, by_ui, sz_ui, sz_ui);
        cv.stroke_rect(boxr, 2.0, theme::ACCENT, false);
        let mid = Rect::new(
            bx_ui + (LOUPE_N / 2) as f32 * z as f32 / sf,
            by_ui + (LOUPE_N / 2) as f32 * z as f32 / sf,
            z as f32 / sf,
            z as f32 / sf,
        );
        cv.stroke_rect(mid, 1.0, Rgba::WHITE, false);
        let col = self.pixel_at(cur).unwrap_or(Rgba::BLACK);
        let text = if self.editor.hex {
            col.to_hex().to_uppercase()
        } else {
            format!("rgb({}, {}, {})", col.r, col.g, col.b)
        };
        cv.pill(bx_ui, by_ui + sz_ui + 4.0, &text);
        cv.pill(
            bx_ui,
            by_ui + sz_ui + 28.0,
            &format!(
                "{}, {}   {}",
                cx,
                cy,
                pinshot_core::i18n::tr("C kopyala · Shift biçim")
            ),
        );
    }
}

/// Copy `region` of `src` (clipped to its bounds).
pub fn crop(src: &Pixmap, region: IRect) -> Option<Pixmap> {
    let r = region.intersect(&IRect::new(0, 0, src.width(), src.height()))?;
    let mut out = Pixmap::new(r.w, r.h)?;
    let sw = src.width() as usize;
    for y in 0..r.h as usize {
        let s = ((r.y as usize + y) * sw + r.x as usize) * 4;
        out.data_mut()[y * r.w as usize * 4..(y + 1) * r.w as usize * 4]
            .copy_from_slice(&src.data()[s..s + r.w as usize * 4]);
    }
    Some(out)
}

/// `src` darkened by the dimming layer, restricted to `area`, into `out` (which has the size of `area`).
pub fn dim_copy(src: &Pixmap, area: IRect, out: &mut Pixmap) {
    let keep = 255 - DIM_ALPHA;
    let lut: [u8; 256] = std::array::from_fn(|v| ((v as u32 * keep + 127) / 255) as u8);
    let (sw, sh) = (src.width() as i64, src.height() as i64);
    let dw = out.width() as usize;
    // the part of `area` that lies inside the source, per row
    let x0 = i64::from(area.x).max(0);
    let x1 = (i64::from(area.x) + i64::from(area.w)).min(sw);
    if x1 <= x0 {
        return;
    }
    let (n, off) = ((x1 - x0) as usize, (x0 - i64::from(area.x)) as usize);
    let src_data = src.data();
    let out_data = out.data_mut();
    for y in 0..area.h as usize {
        let sy = i64::from(area.y) + y as i64;
        if sy < 0 || sy >= sh {
            continue;
        }
        let s = &src_data[((sy * sw + x0) * 4) as usize..][..n * 4];
        let d = &mut out_data[(y * dw + off) * 4..][..n * 4];
        for (d4, s4) in d
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(s.as_chunks::<4>().0)
        {
            *d4 = [
                lut[s4[0] as usize],
                lut[s4[1] as usize],
                lut[s4[2] as usize],
                255,
            ];
        }
    }
}

/// Text box while typing: tinted background, dashed border, text and caret.
fn draw_text_edit(sub: &mut Pixmap, ed: &TextEdit, color: Rgba, px: u32, view: &View) {
    let fonts = Fonts::embedded();
    let ppx = px.max(6) as f32 * view.scale;
    let (ox, oy) = (
        (ed.pos.x - view.origin.x) * view.scale,
        (ed.pos.y - view.origin.y) * view.scale,
    );
    let lines: Vec<&str> = ed.text.split('\n').collect();
    let lh = fonts.line_height(ppx, false);
    let w = lines
        .iter()
        .map(|l| fonts.line_width(l, ppx, false))
        .fold(ppx * 3.0, f32::max);
    let h = (lh * lines.len() as f32).max(ppx * 1.3);
    let mut cv = Canvas::new(sub, 1.0);
    cv.fill_rect(
        Rect::new(ox - 1.0, oy - 1.0, w + 10.0, h + 4.0),
        Rgba::new(255, 255, 255, 40),
    );
    cv.stroke_rect(
        Rect::new(ox - 1.0, oy - 1.0, w + 10.0, h + 4.0),
        1.0,
        color,
        true,
    );
    let item = {
        let mut it = annotate::Item::new(Kind::Text, color, 3, vec![ed.pos]);
        it.text = ed.text.clone();
        it.font_px = px;
        it
    };
    render::render_item(sub, &item, view, 0);
    // caret
    let before: String = ed.text.chars().take(ed.caret).collect();
    let line_no = before.matches('\n').count();
    let col_text = before.rsplit('\n').next().unwrap_or("");
    let cx = ox + fonts.line_width(col_text, ppx, false);
    let cy = oy + lh * line_no as f32;
    let mut cv = Canvas::new(sub, 1.0);
    cv.fill_rect(Rect::new(cx, cy, 1.5, lh), color);
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::screens::Monitor;
    use pinshot_core::editor::{Button, Cmd, Key, Mods};

    fn gradient(w: u32, h: u32) -> Pixmap {
        let mut pm = Pixmap::new(w, h).unwrap();
        for (i, p) in pm.data_mut().chunks_exact_mut(4).enumerate() {
            let (x, y) = (i as u32 % w, i as u32 / w);
            p.copy_from_slice(&[(x % 251) as u8, (y % 251) as u8, 128, 255]);
        }
        pm
    }

    pub fn session() -> Session {
        let caps = vec![
            Capture {
                monitor: Monitor {
                    name: "A".into(),
                    rect: Rect::new(0.0, 0.0, 400.0, 300.0),
                    scale: 1.0,
                },
                pixmap: gradient(400, 300),
            },
            Capture {
                monitor: Monitor {
                    name: "B".into(),
                    rect: Rect::new(400.0, 0.0, 400.0, 300.0),
                    scale: 1.0,
                },
                pixmap: gradient(400, 300),
            },
        ];
        Session::new(caps, Rgba::rgb(255, 0, 0), 3, 20)
    }

    fn geom(mon: usize) -> Geom {
        Geom {
            mon,
            sf: 1.0,
            size: (400, 300),
        }
    }

    fn px(pm: &Pixmap, x: u32, y: u32) -> [u8; 3] {
        let p = pm.pixel(x, y).unwrap();
        [p.red(), p.green(), p.blue()]
    }

    fn drag(s: &mut Session, a: (f32, f32), b: (f32, f32)) {
        let m = &crate::screens::tests_support::NoMeasure;
        s.editor.press(Point::new(a.0, a.1), Button::Left, m);
        s.editor.moved(Point::new(b.0, b.1), false);
        s.editor.release();
        s.sync_toolbar();
    }

    #[test]
    fn outside_the_selection_is_dimmed_and_inside_is_original() {
        let mut s = session();
        s.cursor = None;
        drag(&mut s, (100.0, 100.0), (200.0, 180.0));
        let full = IRect::new(0, 0, 400, 300);
        let f = s.compose(&geom(0), full);
        let orig = |x: u32, y: u32| px(&s.caps[0].pixmap, x, y);
        assert_eq!(px(&f, 150, 140), orig(150, 140), "inside: untouched");
        let out = px(&f, 300, 250);
        assert!(
            out[0] < orig(300, 250)[0] && out[1] < orig(300, 250)[1],
            "outside: darker"
        );
        // blue channel 128 dimmed by 120/255 → ~68
        assert!((out[2] as i32 - 68).abs() <= 2);
        assert_eq!(
            px(&f, 100, 125),
            [0x0a, 0x84, 0xff],
            "accent border on the selection's left edge"
        );
    }

    #[test]
    fn damage_frame_matches_the_same_area_of_a_full_frame() {
        let mut s = session();
        drag(&mut s, (100.0, 100.0), (250.0, 200.0));
        s.editor.set_tool(
            Tool::Draw(Kind::Rect),
            &crate::screens::tests_support::NoMeasure,
        );
        drag(&mut s, (120.0, 120.0), (200.0, 160.0));
        let full = s.compose(&geom(0), IRect::new(0, 0, 400, 300));
        let part_rect = IRect::new(90, 90, 80, 60);
        let part = s.compose(&geom(0), part_rect);
        let mut differing = 0;
        for y in 0..part_rect.h {
            for x in 0..part_rect.w {
                let (a, b) = (px(&part, x, y), px(&full, x + 90, y + 90));
                // anti-aliased corners may differ by a rounding step depending on where the damage window starts
                if a.iter()
                    .zip(b)
                    .any(|(p, q)| (*p as i32 - q as i32).abs() > 4)
                {
                    differing += 1;
                }
            }
        }
        assert!(differing <= 3, "{differing} pixels differ");
    }

    #[test]
    fn selection_across_two_monitors_is_split_between_windows() {
        let mut s = session();
        drag(&mut s, (350.0, 100.0), (450.0, 180.0));
        let a = s.compose(&geom(0), IRect::new(0, 0, 400, 300));
        let b = s.compose(&geom(1), IRect::new(0, 0, 400, 300));
        assert_eq!(px(&a, 380, 140), px(&s.caps[0].pixmap, 380, 140));
        assert_eq!(px(&b, 20, 140), px(&s.caps[1].pixmap, 20, 140));
        assert_ne!(
            px(&b, 200, 140),
            px(&s.caps[1].pixmap, 200, 140),
            "outside the selection on B is dimmed"
        );
        assert_eq!(
            s.host(),
            Some(1),
            "the toolbar lives where the bottom-right corner is"
        );
        assert!(s.toolbar_origin(&geom(0)).is_none() && s.toolbar_origin(&geom(1)).is_some());
    }

    #[test]
    fn annotations_appear_in_preview_and_final_image_identically() {
        let mut s = session();
        drag(&mut s, (100.0, 100.0), (300.0, 250.0));
        let m = &crate::screens::tests_support::NoMeasure;
        s.editor.set_tool(Tool::Draw(Kind::Rect), m);
        s.editor.fill = true;
        s.editor.color = Rgba::rgb(0, 255, 0);
        drag(&mut s, (150.0, 150.0), (200.0, 200.0));
        let frame = s.compose(&geom(0), IRect::new(0, 0, 400, 300));
        let (out, scale) = s.render_selection().unwrap();
        assert_eq!(scale, 1.0);
        assert_eq!((out.width(), out.height()), (200, 150));
        for (fx, fy) in [(160u32, 160u32), (199, 199), (110, 110), (250, 240)] {
            assert_eq!(
                px(&frame, fx, fy),
                px(&out, fx - 100, fy - 100),
                "at {fx},{fy}"
            );
        }
        assert_eq!(px(&out, 60, 60), [0, 255, 0]);
    }

    #[test]
    fn extent_covers_selection_toolbar_and_loupe_only() {
        let mut s = session();
        let g = geom(0);
        assert!(
            s.extent(&g).is_none(),
            "the hint pill is redrawn on state changes only, not with every frame"
        );
        assert_eq!(s.hint_strip(&g).w, g.size.0);
        s.cursor = Some(Point::new(200.0, 150.0));
        let with_loupe = s.extent(&g).unwrap();
        assert!(with_loupe.w > 100);
        drag(&mut s, (100.0, 100.0), (150.0, 140.0));
        s.cursor = None;
        let e = s.extent(&g).unwrap();
        assert!(e.x <= 100 && e.right() >= 150);
        assert!(
            s.extent(&geom(1)).is_none(),
            "the other monitor has nothing to redraw"
        );
        let _ = (Cmd::None, Key::Escape, Mods::default());
    }

    #[test]
    fn loupe_shows_zoomed_pixels_and_the_colour_text() {
        let mut s = session();
        s.cursor = Some(Point::new(200.0, 100.0));
        let f = s.compose(&geom(0), IRect::new(0, 0, 400, 300));
        // loupe box starts at cursor + 22
        let (bx, by) = (222u32, 122u32);
        assert_eq!(
            px(&f, bx + 7 * 8 + 3, by + 7 * 8 + 3),
            px(&s.caps[0].pixmap, 200, 100),
            "centre cell = pixel under the cursor"
        );
        assert_eq!(
            s.pixel_at(Point::new(200.0, 100.0)),
            Some(Rgba::rgb(200, 100, 128))
        );
    }

    #[test]
    fn text_edit_box_is_drawn_while_typing() {
        let mut s = session();
        let m = &crate::screens::tests_support::NoMeasure;
        drag(&mut s, (100.0, 100.0), (300.0, 250.0));
        s.editor.set_tool(Tool::Draw(Kind::Text), m);
        s.editor.press(Point::new(150.0, 150.0), Button::Left, m);
        s.editor.release();
        for c in "Hi".chars() {
            s.editor.key(Key::Char(c), Mods::default(), m);
        }
        let with = s.compose(&geom(0), IRect::new(0, 0, 400, 300));
        s.editor.key(Key::Escape, Mods::default(), m);
        let committed = s.compose(&geom(0), IRect::new(0, 0, 400, 300));
        let text_px = |f: &Pixmap| {
            (150..200)
                .flat_map(|x| (150..180).map(move |y| (x, y)))
                .filter(|&(x, y)| px(f, x, y) == [255, 0, 0])
                .count()
        };
        assert!(
            text_px(&with) > 20 && text_px(&committed) > 20,
            "red glyph pixels in both states"
        );
        assert_eq!(s.editor.items.len(), 1);
    }
}
