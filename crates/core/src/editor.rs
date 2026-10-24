//! The capture editor as a window-system-independent state machine: selection, tools, drawing, moving, inline text
//! and keyboard commands. The UI forwards pointer/keyboard events here and draws what it exposes.
//!
//! All coordinates are logical desktop coordinates, shared by every monitor.

use crate::annotate::{self, Item, Kind, HIT_TOLERANCE};
use crate::color::Rgba;
use crate::geom::{Point, Rect};
use crate::history::{Geom, History};
use crate::text::TextMeasure;

pub const HANDLE_SIZE: f32 = 8.0;
pub const MIN_SELECTION: f32 = 4.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Select,
    Draw(Kind),
}

impl Tool {
    pub fn from_key(c: char) -> Option<Tool> {
        Some(match c {
            'v' => Tool::Select,
            'r' => Tool::Draw(Kind::Rect),
            'e' => Tool::Draw(Kind::Ellipse),
            'l' => Tool::Draw(Kind::Line),
            'a' => Tool::Draw(Kind::Arrow),
            'p' => Tool::Draw(Kind::Pen),
            'h' => Tool::Draw(Kind::Marker),
            't' => Tool::Draw(Kind::Text),
            'n' => Tool::Draw(Kind::Number),
            'm' => Tool::Draw(Kind::Mosaic),
            'b' => Tool::Draw(Kind::Blur),
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Escape,
    Enter,
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    F3,
    Shift,
    Char(char),
}

/// `ctrl` means Command on macOS (the platform layer maps it).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
}

/// What the editor asks the application to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Cmd {
    None,
    Copy,
    Save,
    Pin,
    Ocr,
    Close,
    /// Copy the colour under the cursor (the app knows the pixels); `hex` selects the text format.
    CopyColor {
        hex: bool,
    },
    /// Select the whole monitor under the cursor.
    SelectMonitor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Idle,
    SelNew,
    SelMove,
    SelResize,
    Draw,
    ItemMove,
    ItemHandle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorKind {
    Cross,
    Arrow,
    Text,
    Hand,
    Move,
    SizeFDiag,
    SizeBDiag,
    SizeVer,
    SizeHor,
}

/// Inline text being typed.
#[derive(Clone, Debug)]
pub struct TextEdit {
    /// `Some` when an existing item is edited.
    pub item: Option<u64>,
    pub pos: Point,
    pub text: String,
    /// Caret as a character index.
    pub caret: usize,
    before: Option<Geom>,
}

impl TextEdit {
    fn byte(&self, chars: usize) -> usize {
        self.text
            .char_indices()
            .nth(chars)
            .map(|(i, _)| i)
            .unwrap_or(self.text.len())
    }

    fn len(&self) -> usize {
        self.text.chars().count()
    }

    pub fn insert(&mut self, s: &str) {
        let at = self.byte(self.caret);
        self.text.insert_str(at, s);
        self.caret += s.chars().count();
    }

    fn line_bounds(&self) -> (usize, usize) {
        let chars: Vec<char> = self.text.chars().collect();
        let start = chars[..self.caret]
            .iter()
            .rposition(|&c| c == '\n')
            .map(|i| i + 1)
            .unwrap_or(0);
        let end = chars[self.caret..]
            .iter()
            .position(|&c| c == '\n')
            .map(|i| self.caret + i)
            .unwrap_or(chars.len());
        (start, end)
    }

    /// Returns true if the key was consumed by the text box.
    fn key(&mut self, key: Key, mods: Mods) -> bool {
        match key {
            Key::Backspace if self.caret > 0 => {
                let (a, b) = (self.byte(self.caret - 1), self.byte(self.caret));
                self.text.replace_range(a..b, "");
                self.caret -= 1;
            }
            Key::Delete if self.caret < self.len() => {
                let (a, b) = (self.byte(self.caret), self.byte(self.caret + 1));
                self.text.replace_range(a..b, "");
            }
            Key::Left => self.caret = self.caret.saturating_sub(1),
            Key::Right => self.caret = (self.caret + 1).min(self.len()),
            Key::Home => self.caret = self.line_bounds().0,
            Key::End => self.caret = self.line_bounds().1,
            Key::Up | Key::Down => {
                let (s, e) = self.line_bounds();
                let col = self.caret - s;
                let chars: Vec<char> = self.text.chars().collect();
                if key == Key::Up && s > 0 {
                    let ps = chars[..s - 1]
                        .iter()
                        .rposition(|&c| c == '\n')
                        .map(|i| i + 1)
                        .unwrap_or(0);
                    self.caret = ps + col.min(s - 1 - ps);
                } else if key == Key::Down && e < chars.len() {
                    let ne = chars[e + 1..]
                        .iter()
                        .position(|&c| c == '\n')
                        .map(|i| e + 1 + i)
                        .unwrap_or(chars.len());
                    self.caret = e + 1 + col.min(ne - (e + 1));
                }
            }
            Key::Enter if mods.shift => self.insert("\n"),
            Key::Char(c) if !mods.ctrl && !c.is_control() => self.insert(&c.to_string()),
            Key::Backspace | Key::Delete => {}
            _ => return false,
        }
        true
    }
}

pub struct Editor {
    /// Union of all monitors.
    pub bounds: Rect,
    pub sel: Option<Rect>,
    pub items: Vec<Item>,
    pub history: History,
    pub tool: Tool,
    pub selected: Option<u64>,
    pub color: Rgba,
    pub pen_w: u32,
    pub fill: bool,
    pub dashed: bool,
    pub font_px: u32,
    pub mode: Mode,
    pub cur: Option<Item>,
    pub edit: Option<TextEdit>,
    /// Colour readout format for the pre-selection loupe.
    pub hex: bool,
    anchor: Point,
    last: Point,
    grab_off: Point,
    sel_handle: [i8; 2],
    spec: annotate::Handle,
    before: Option<Geom>,
}

impl Editor {
    pub fn new(bounds: Rect, color: Rgba, pen_w: u32, font_px: u32) -> Self {
        Editor {
            bounds,
            sel: None,
            items: Vec::new(),
            history: History::default(),
            tool: Tool::Select,
            selected: None,
            color,
            pen_w,
            fill: false,
            dashed: false,
            font_px,
            mode: Mode::Idle,
            cur: None,
            edit: None,
            hex: true,
            anchor: Point::default(),
            last: Point::default(),
            grab_off: Point::default(),
            sel_handle: [0, 0],
            spec: (None, None),
            before: None,
        }
    }

    // ------------------------------------------------------------------ queries
    pub fn selected_item(&self) -> Option<&Item> {
        self.selected
            .and_then(|id| self.items.iter().find(|i| i.id == id))
    }

    fn selected_mut(&mut self) -> Option<&mut Item> {
        let id = self.selected?;
        self.items.iter_mut().find(|i| i.id == id)
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// Selection handles: (handle id as [-1|0|1; 2], rectangle).
    pub fn sel_handles(&self) -> Vec<([i8; 2], Rect)> {
        let Some(s) = self.sel else { return Vec::new() };
        let mut out = Vec::new();
        for (iy, y) in [(-1, s.y), (0, s.y + s.h / 2.0), (1, s.bottom())] {
            for (ix, x) in [(-1, s.x), (0, s.x + s.w / 2.0), (1, s.right())] {
                if ix == 0 && iy == 0 {
                    continue;
                }
                out.push((
                    [ix, iy],
                    Rect::new(
                        x - HANDLE_SIZE / 2.0,
                        y - HANDLE_SIZE / 2.0,
                        HANDLE_SIZE,
                        HANDLE_SIZE,
                    ),
                ));
            }
        }
        out
    }

    fn sel_handle_at(&self, pt: Point) -> Option<[i8; 2]> {
        self.sel_handles()
            .into_iter()
            .find(|(_, r)| r.inflated(4.0).contains(pt))
            .map(|(h, _)| h)
    }

    fn item_at(&self, pt: Point, m: &dyn TextMeasure) -> Option<u64> {
        let sel = self.sel?;
        if !sel.contains(pt) {
            return None;
        }
        self.items
            .iter()
            .rev()
            .find(|it| annotate::hit_test(it, pt, HIT_TOLERANCE, m))
            .map(|it| it.id)
    }

    fn item_handle_at(&self, pt: Point) -> Option<annotate::Handle> {
        let it = self.selected_item()?;
        annotate::handles(it)
            .into_iter()
            .find(|(p, _)| (p.x - pt.x).abs() <= 8.0 && (p.y - pt.y).abs() <= 8.0)
            .map(|(_, s)| s)
    }

    pub fn cursor_at(&self, pt: Point, m: &dyn TextMeasure) -> CursorKind {
        let Some(sel) = self.sel else {
            return CursorKind::Cross;
        };
        if let Some(h) = self.sel_handle_at(pt) {
            return match h {
                [-1, -1] | [1, 1] => CursorKind::SizeFDiag,
                [1, -1] | [-1, 1] => CursorKind::SizeBDiag,
                [0, _] => CursorKind::SizeVer,
                _ => CursorKind::SizeHor,
            };
        }
        match self.tool {
            Tool::Select => {
                if self.item_handle_at(pt).is_some() {
                    CursorKind::Hand
                } else if self.item_at(pt, m).is_some() {
                    CursorKind::Move
                } else if sel.contains(pt) {
                    CursorKind::Hand
                } else {
                    CursorKind::Arrow
                }
            }
            _ if !sel.contains(pt) => CursorKind::Arrow,
            Tool::Draw(Kind::Text) => CursorKind::Text,
            _ => CursorKind::Cross,
        }
    }

    /// Which property controls the toolbar shows right now (`None` hides the row).
    pub fn property_kind(&self) -> Option<Kind> {
        match self.tool {
            Tool::Select => self.selected_item().map(|i| i.kind),
            Tool::Draw(k) => Some(k),
        }
    }

    pub fn is_selecting(&self) -> bool {
        matches!(self.mode, Mode::SelNew | Mode::SelMove | Mode::SelResize)
    }

    // ------------------------------------------------------------------ property edits (not undoable)
    pub fn set_tool(&mut self, tool: Tool, m: &dyn TextMeasure) {
        self.commit_text(m);
        self.tool = tool;
        if tool != Tool::Select {
            self.selected = None;
        }
    }

    fn target(&mut self) -> Option<&mut Item> {
        if self.tool == Tool::Select {
            self.selected_mut()
        } else {
            None
        }
    }

    pub fn set_color(&mut self, c: Rgba) {
        self.color = c;
        if let Some(t) = self.target() {
            t.color = c;
        }
    }

    pub fn set_width(&mut self, w: u32) {
        match self.target() {
            Some(t) => t.width = w,
            None => self.pen_w = w,
        }
    }

    pub fn set_fill(&mut self, v: bool) {
        match self.target() {
            Some(t) => t.fill = v,
            None => self.fill = v,
        }
    }

    pub fn set_dashed(&mut self, v: bool) {
        match self.target() {
            Some(t) => t.dashed = v,
            None => self.dashed = v,
        }
    }

    pub fn set_font_px(&mut self, v: u32) {
        match self.target() {
            Some(t) if t.kind == Kind::Text => t.font_px = v,
            _ => self.font_px = v,
        }
    }

    // ------------------------------------------------------------------ items
    fn new_item(&self, kind: Kind, pts: Vec<Point>) -> Item {
        let mut it = Item::new(kind, self.color, self.pen_w, pts);
        it.fill = self.fill;
        it.dashed = self.dashed;
        it.font_px = self.font_px;
        it
    }

    fn add(&mut self, it: Item) {
        let id = it.id;
        self.items.push(it);
        self.history.added(id, self.items.len() - 1);
    }

    pub fn delete_selected(&mut self) {
        if let Some(id) = self.selected {
            if let Some(pos) = self.items.iter().position(|i| i.id == id) {
                let it = self.items.remove(pos);
                self.history.deleted(it, pos);
                self.selected = None;
            }
        }
    }

    pub fn undo(&mut self, m: &dyn TextMeasure) {
        self.commit_text(m);
        self.history.undo(&mut self.items);
        self.drop_stale_selection();
    }

    pub fn redo(&mut self, m: &dyn TextMeasure) {
        self.commit_text(m);
        self.history.redo(&mut self.items);
        self.drop_stale_selection();
    }

    fn drop_stale_selection(&mut self) {
        if let Some(id) = self.selected {
            if !self.items.iter().any(|i| i.id == id) {
                self.selected = None;
            }
        }
    }

    pub fn select_rect(&mut self, r: Rect) {
        if self.sel.map(|s| s.is_empty()).unwrap_or(true) {
            self.sel = Some(r.intersection(&self.bounds));
        }
    }

    fn clamp(&self, r: Rect) -> Rect {
        r.intersection(&self.bounds)
    }

    // ------------------------------------------------------------------ pointer
    pub fn press(&mut self, pt: Point, button: Button, m: &dyn TextMeasure) -> Cmd {
        if self.edit.is_some() {
            self.commit_text(m);
            return Cmd::None;
        }
        if button == Button::Right {
            return self.right_click();
        }
        let Some(sel) = self.sel else {
            self.mode = Mode::SelNew;
            self.anchor = pt;
            self.sel = Some(Rect::from_points(pt, pt));
            return Cmd::None;
        };
        if let Some(h) = self.sel_handle_at(pt) {
            self.mode = Mode::SelResize;
            self.sel_handle = h;
            return Cmd::None;
        }
        if self.tool == Tool::Select {
            if let Some(spec) = self.item_handle_at(pt) {
                self.mode = Mode::ItemHandle;
                self.spec = spec;
                self.before = self.selected_item().map(Geom::of);
                return Cmd::None;
            }
            if let Some(id) = self.item_at(pt, m) {
                self.selected = Some(id);
                self.mode = Mode::ItemMove;
                self.last = pt;
                self.before = self.selected_item().map(Geom::of);
                return Cmd::None;
            }
            if sel.contains(pt) {
                self.selected = None;
                self.mode = Mode::SelMove;
                self.grab_off = Point::new(pt.x - sel.x, pt.y - sel.y);
            }
            return Cmd::None;
        }
        if !sel.contains(pt) {
            return Cmd::None;
        }
        let Tool::Draw(kind) = self.tool else {
            return Cmd::None;
        };
        match kind {
            Kind::Text => self.begin_text(pt, None),
            Kind::Number => {
                let it = self.new_item(Kind::Number, vec![pt]);
                self.add(it);
            }
            _ => {
                let pts = if kind.is_freehand() {
                    vec![pt]
                } else {
                    vec![pt, pt]
                };
                self.mode = Mode::Draw;
                self.anchor = pt;
                self.cur = Some(self.new_item(kind, pts));
            }
        }
        Cmd::None
    }

    pub fn moved(&mut self, pt: Point, shift: bool) {
        match self.mode {
            Mode::SelNew => self.sel = Some(self.clamp(Rect::from_points(self.anchor, pt))),
            Mode::SelMove => {
                if let Some(s) = self.sel {
                    let b = self.bounds;
                    let x = (pt.x - self.grab_off.x).clamp(b.x, (b.right() - s.w).max(b.x));
                    let y = (pt.y - self.grab_off.y).clamp(b.y, (b.bottom() - s.h).max(b.y));
                    self.sel = Some(Rect::new(x, y, s.w, s.h));
                }
            }
            Mode::SelResize => {
                if let Some(s) = self.sel {
                    let b = self.bounds;
                    let (x, y) = (pt.x.clamp(b.x, b.right()), pt.y.clamp(b.y, b.bottom()));
                    let (mut l, mut t, mut r, mut bt) = (s.x, s.y, s.right(), s.bottom());
                    match self.sel_handle[0] {
                        -1 => l = x,
                        1 => r = x,
                        _ => {}
                    }
                    match self.sel_handle[1] {
                        -1 => t = y,
                        1 => bt = y,
                        _ => {}
                    }
                    // dragging past the opposite edge flips the handle so the rectangle stays valid
                    self.sel = Some(Rect::from_points(Point::new(l, t), Point::new(r, bt)));
                    if l > r {
                        self.sel_handle[0] = -self.sel_handle[0];
                    }
                    if t > bt {
                        self.sel_handle[1] = -self.sel_handle[1];
                    }
                }
            }
            Mode::Draw => {
                let anchor = self.anchor;
                if let Some(c) = &mut self.cur {
                    if c.kind.is_freehand() {
                        c.pts.push(pt);
                    } else {
                        let end = if shift {
                            annotate::constrain(c.kind, anchor, pt)
                        } else {
                            pt
                        };
                        *c.pts.last_mut().expect("points") = end;
                    }
                }
            }
            Mode::ItemMove => {
                let (dx, dy) = (pt.x - self.last.x, pt.y - self.last.y);
                self.last = pt;
                if let Some(it) = self.selected_mut() {
                    it.translate(dx, dy);
                }
            }
            Mode::ItemHandle => {
                let spec = self.spec;
                if let Some(it) = self.selected_mut() {
                    annotate::drag_handle(it, spec, pt, shift);
                }
            }
            Mode::Idle => {}
        }
    }

    pub fn release(&mut self) {
        let mode = std::mem::replace(&mut self.mode, Mode::Idle);
        match mode {
            Mode::SelNew => {
                if self
                    .sel
                    .map(|s| s.w < MIN_SELECTION || s.h < MIN_SELECTION)
                    .unwrap_or(false)
                {
                    self.sel = None;
                }
            }
            Mode::Draw => {
                if let Some(it) = self.cur.take() {
                    let r = it.rect();
                    if it.kind.is_freehand() || it.kind.is_line_like() || r.w >= 3.0 || r.h >= 3.0 {
                        self.add(it);
                    }
                }
            }
            Mode::ItemMove | Mode::ItemHandle => {
                if let (Some(before), Some(id)) = (self.before.take(), self.selected) {
                    if let Some(it) = self.items.iter().find(|i| i.id == id) {
                        self.history.modified(it, before);
                    }
                }
            }
            _ => {}
        }
    }

    pub fn double_click(&mut self, pt: Point, m: &dyn TextMeasure) -> Cmd {
        let hit = self.item_at(pt, m);
        if let Some(id) = hit {
            if self.tool == Tool::Select
                && self
                    .items
                    .iter()
                    .any(|i| i.id == id && i.kind == Kind::Text)
            {
                let pos = self
                    .items
                    .iter()
                    .find(|i| i.id == id)
                    .map(|i| i.first())
                    .unwrap_or(pt);
                self.begin_text(pos, Some(id));
            }
            return Cmd::None;
        }
        match self.sel {
            Some(s) if self.tool == Tool::Select && s.contains(pt) => Cmd::Copy,
            _ => Cmd::None,
        }
    }

    fn right_click(&mut self) -> Cmd {
        if self.tool != Tool::Select {
            self.tool = Tool::Select;
        } else if self.selected.is_some() {
            self.selected = None;
        } else if self.sel.is_some() {
            self.sel = None;
            self.items.clear();
            self.history.clear();
        } else {
            return Cmd::Close;
        }
        Cmd::None
    }

    // ------------------------------------------------------------------ inline text
    fn begin_text(&mut self, pos: Point, item: Option<u64>) {
        let (text, before, pos) = match item.and_then(|id| self.items.iter().find(|i| i.id == id)) {
            Some(it) => (it.text.clone(), Some(Geom::of(it)), it.first()),
            None => (String::new(), None, pos),
        };
        let caret = text.chars().count();
        self.edit = Some(TextEdit {
            item,
            pos,
            text,
            caret,
            before,
        });
    }

    /// Look (colour, pixel height) of the text being typed.
    pub fn edit_look(&self) -> (Rgba, u32) {
        match self
            .edit
            .as_ref()
            .and_then(|e| e.item)
            .and_then(|id| self.items.iter().find(|i| i.id == id))
        {
            Some(it) => (it.color, it.font_px),
            None => (self.color, self.font_px),
        }
    }

    pub fn commit_text(&mut self, _m: &dyn TextMeasure) {
        let Some(ed) = self.edit.take() else { return };
        let text = ed.text.trim_end().to_string();
        match ed.item {
            Some(id) => {
                if text.is_empty() {
                    if let Some(pos) = self.items.iter().position(|i| i.id == id) {
                        let it = self.items.remove(pos);
                        self.history.deleted(it, pos);
                        self.selected = None;
                    }
                } else if let (Some(it), Some(before)) =
                    (self.items.iter_mut().find(|i| i.id == id), ed.before)
                {
                    it.text = text;
                    let snapshot = it.clone();
                    self.history.modified(&snapshot, before);
                }
            }
            None if !text.is_empty() => {
                let mut it = self.new_item(Kind::Text, vec![ed.pos]);
                it.text = text;
                let id = it.id;
                self.add(it);
                self.selected = Some(id);
                self.tool = Tool::Select;
            }
            None => {}
        }
    }

    // ------------------------------------------------------------------ keyboard
    /// Keyboard input. While text is being typed everything goes to the text box.
    pub fn key(&mut self, key: Key, mods: Mods, m: &dyn TextMeasure) -> Cmd {
        if let Some(ed) = &mut self.edit {
            match key {
                Key::Escape => self.commit_text(m),
                Key::Enter if !mods.shift => self.commit_text(m),
                _ => {
                    ed.key(key, mods);
                }
            }
            return Cmd::None;
        }
        let (ctrl, shift) = (mods.ctrl, mods.shift);
        match key {
            Key::Escape => {
                if self.tool != Tool::Select {
                    self.tool = Tool::Select;
                } else if self.selected.is_some() {
                    self.selected = None;
                } else {
                    return Cmd::Close;
                }
            }
            Key::Shift if self.sel.is_none() => self.hex = !self.hex,
            Key::Char('a') if ctrl => return Cmd::SelectMonitor,
            _ if self.sel.is_none() => {
                if key == Key::Char('c') && !ctrl {
                    return Cmd::CopyColor { hex: self.hex };
                }
            }
            Key::Enter => return Cmd::Copy,
            Key::Char('c') if ctrl => return Cmd::Copy,
            Key::Char('z') if ctrl && !shift => self.undo(m),
            Key::Char('y') if ctrl => self.redo(m),
            Key::Char('z') if ctrl && shift => self.redo(m),
            Key::Char('s') if ctrl => return Cmd::Save,
            Key::Delete | Key::Backspace => self.delete_selected(),
            Key::F3 => return Cmd::Pin,
            Key::Char('p') if ctrl => return Cmd::Pin,
            Key::Left | Key::Right | Key::Up | Key::Down => {
                let step = if shift { 10.0 } else { 1.0 };
                let (dx, dy) = match key {
                    Key::Left => (-step, 0.0),
                    Key::Right => (step, 0.0),
                    Key::Up => (0.0, -step),
                    _ => (0.0, step),
                };
                if self.selected.is_some() && self.tool == Tool::Select {
                    if let Some(before) = self.selected_item().map(Geom::of) {
                        if let Some(it) = self.selected_mut() {
                            it.translate(dx, dy);
                        }
                        if let Some(it) = self.selected_item().cloned() {
                            self.history.modified(&it, before);
                        }
                    }
                } else if let Some(s) = self.sel {
                    let moved = s.translated(dx, dy);
                    // keep the size: slide inside the bounds instead of shrinking at the edge
                    let b = self.bounds;
                    let x = moved.x.clamp(b.x, (b.right() - s.w).max(b.x));
                    let y = moved.y.clamp(b.y, (b.bottom() - s.h).max(b.y));
                    self.sel = Some(Rect::new(x, y, s.w, s.h));
                }
            }
            Key::Char(c) if !ctrl => {
                if let Some(tool) = Tool::from_key(c) {
                    self.set_tool(tool, m);
                } else if c == 'o' {
                    return Cmd::Ocr;
                }
            }
            _ => {}
        }
        Cmd::None
    }

    /// Items to draw: committed ones plus the one being drawn, without the one being text-edited.
    pub fn visible_items(&self) -> Vec<Item> {
        let skip = self.edit.as_ref().and_then(|e| e.item);
        self.items
            .iter()
            .filter(|i| Some(i.id) != skip)
            .cloned()
            .chain(self.cur.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake;
    impl TextMeasure for Fake {
        fn measure(&self, text: &str, _px: f32, _b: bool) -> (f32, f32) {
            let lines: Vec<&str> = text.split('\n').collect();
            (
                lines.iter().map(|l| l.chars().count()).max().unwrap_or(0) as f32 * 10.0,
                lines.len() as f32 * 20.0,
            )
        }
    }

    fn ed() -> Editor {
        Editor::new(
            Rect::new(0.0, 0.0, 1000.0, 800.0),
            Rgba::rgb(255, 0, 0),
            3,
            20,
        )
    }

    fn drag(e: &mut Editor, a: (f32, f32), b: (f32, f32)) {
        e.press(Point::new(a.0, a.1), Button::Left, &Fake);
        e.moved(Point::new((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0), false);
        e.moved(Point::new(b.0, b.1), false);
        e.release();
    }

    fn typed(e: &mut Editor, s: &str) {
        for c in s.chars() {
            e.key(Key::Char(c), Mods::default(), &Fake);
        }
    }

    fn with_selection() -> Editor {
        let mut e = ed();
        drag(&mut e, (100.0, 100.0), (500.0, 400.0));
        e
    }

    #[test]
    fn dragging_makes_a_selection_and_tiny_drags_cancel_it() {
        let e = with_selection();
        assert_eq!(e.sel, Some(Rect::new(100.0, 100.0, 400.0, 300.0)));
        let mut t = ed();
        drag(&mut t, (10.0, 10.0), (12.0, 12.0));
        assert_eq!(t.sel, None);
        // drag beyond the desktop is clamped
        let mut c = ed();
        drag(&mut c, (900.0, 700.0), (2000.0, 2000.0));
        assert_eq!(c.sel, Some(Rect::new(900.0, 700.0, 100.0, 100.0)));
    }

    #[test]
    fn selection_can_be_moved_and_resized_by_handles() {
        let mut e = with_selection();
        e.press(Point::new(200.0, 200.0), Button::Left, &Fake); // inside, select tool: move
        e.moved(Point::new(250.0, 230.0), false);
        e.release();
        assert_eq!(e.sel, Some(Rect::new(150.0, 130.0, 400.0, 300.0)));
        // bottom-right handle at (550, 430)
        e.press(Point::new(550.0, 430.0), Button::Left, &Fake);
        e.moved(Point::new(650.0, 480.0), false);
        e.release();
        assert_eq!(e.sel, Some(Rect::new(150.0, 130.0, 500.0, 350.0)));
        // moving cannot leave the desktop
        e.press(Point::new(300.0, 300.0), Button::Left, &Fake);
        e.moved(Point::new(5000.0, 5000.0), false);
        e.release();
        let s = e.sel.unwrap();
        assert!(s.right() <= 1000.0 && s.bottom() <= 800.0 && (s.w, s.h) == (500.0, 350.0));
    }

    #[test]
    fn dragging_a_handle_past_the_opposite_edge_flips_instead_of_collapsing() {
        let mut e = with_selection();
        e.press(Point::new(500.0, 400.0), Button::Left, &Fake);
        e.moved(Point::new(50.0, 450.0), false);
        e.release();
        let s = e.sel.unwrap();
        assert_eq!((s.x, s.right()), (50.0, 100.0));
    }

    #[test]
    fn drawing_with_a_tool_adds_undoable_items_only_inside_the_selection() {
        let mut e = with_selection();
        e.key(Key::Char('r'), Mods::default(), &Fake);
        assert_eq!(e.tool, Tool::Draw(Kind::Rect));
        drag(&mut e, (150.0, 150.0), (300.0, 250.0));
        assert_eq!(e.items.len(), 1);
        drag(&mut e, (700.0, 700.0), (800.0, 780.0)); // outside: ignored
        assert_eq!(e.items.len(), 1);
        drag(&mut e, (200.0, 200.0), (201.0, 201.0)); // too small
        assert_eq!(e.items.len(), 1);
        assert!(e.can_undo());
        e.key(
            Key::Char('z'),
            Mods {
                ctrl: true,
                ..Mods::default()
            },
            &Fake,
        );
        assert!(e.items.is_empty() && e.can_redo());
        e.key(
            Key::Char('y'),
            Mods {
                ctrl: true,
                ..Mods::default()
            },
            &Fake,
        );
        assert_eq!(e.items.len(), 1);
    }

    #[test]
    fn shift_keeps_shapes_square() {
        let mut e = with_selection();
        e.set_tool(Tool::Draw(Kind::Rect), &Fake);
        e.press(Point::new(150.0, 150.0), Button::Left, &Fake);
        e.moved(Point::new(300.0, 200.0), true);
        e.release();
        let r = e.items[0].rect();
        assert_eq!(r.w, r.h);
    }

    #[test]
    fn select_tool_moves_items_and_records_one_undo_step() {
        let mut e = with_selection();
        e.set_tool(Tool::Draw(Kind::Rect), &Fake);
        e.key(Key::Char('f'), Mods::default(), &Fake); // not a tool key: nothing
        drag(&mut e, (150.0, 150.0), (300.0, 250.0));
        e.set_tool(Tool::Select, &Fake);
        e.press(Point::new(150.0, 200.0), Button::Left, &Fake); // on the left edge
        e.moved(Point::new(170.0, 210.0), false);
        e.moved(Point::new(180.0, 220.0), false);
        e.release();
        assert_eq!(e.items[0].first(), Point::new(180.0, 170.0));
        e.undo(&Fake);
        assert_eq!(e.items[0].first(), Point::new(150.0, 150.0));
        e.undo(&Fake);
        assert!(e.items.is_empty());
    }

    #[test]
    fn item_handles_resize_and_delete_works() {
        let mut e = with_selection();
        e.set_tool(Tool::Draw(Kind::Rect), &Fake);
        drag(&mut e, (150.0, 150.0), (300.0, 250.0));
        e.set_tool(Tool::Select, &Fake);
        e.press(Point::new(150.0, 200.0), Button::Left, &Fake);
        e.release();
        assert!(e.selected.is_some());
        e.press(Point::new(300.0, 250.0), Button::Left, &Fake); // bottom-right handle
        e.moved(Point::new(340.0, 280.0), false);
        e.release();
        assert_eq!(e.items[0].rect(), Rect::new(150.0, 150.0, 190.0, 130.0));
        e.key(Key::Delete, Mods::default(), &Fake);
        assert!(e.items.is_empty() && e.selected.is_none());
        e.undo(&Fake);
        assert_eq!(e.items.len(), 1);
    }

    #[test]
    fn number_tool_places_badges_on_click_and_pen_collects_points() {
        let mut e = with_selection();
        e.set_tool(Tool::Draw(Kind::Number), &Fake);
        e.press(Point::new(200.0, 200.0), Button::Left, &Fake);
        e.release();
        e.press(Point::new(260.0, 200.0), Button::Left, &Fake);
        e.release();
        assert_eq!(e.items.iter().filter(|i| i.kind == Kind::Number).count(), 2);
        e.set_tool(Tool::Draw(Kind::Pen), &Fake);
        drag(&mut e, (300.0, 300.0), (320.0, 330.0));
        assert!(e.items[2].pts.len() >= 3);
    }

    #[test]
    fn text_tool_types_commits_and_edits() {
        let mut e = with_selection();
        e.set_tool(Tool::Draw(Kind::Text), &Fake);
        e.press(Point::new(200.0, 200.0), Button::Left, &Fake);
        e.release();
        typed(&mut e, "Merhaba");
        e.key(Key::Backspace, Mods::default(), &Fake);
        e.key(
            Key::Enter,
            Mods {
                shift: true,
                ..Mods::default()
            },
            &Fake,
        );
        typed(&mut e, "ğü");
        e.key(Key::Enter, Mods::default(), &Fake);
        assert!(e.edit.is_none());
        assert_eq!(e.items[0].text, "Merhab\nğü");
        assert_eq!(
            e.tool,
            Tool::Select,
            "back to select with the new text selected"
        );
        assert_eq!(e.selected, Some(e.items[0].id));
        // edit again by double click
        e.double_click(Point::new(205.0, 205.0), &Fake);
        assert!(e.edit.is_some());
        typed(&mut e, "!");
        e.key(Key::Escape, Mods::default(), &Fake);
        assert_eq!(e.items[0].text, "Merhab\nğü!");
        // clearing the text removes the item; undo brings it back
        e.double_click(Point::new(205.0, 205.0), &Fake);
        for _ in 0..20 {
            e.key(Key::Backspace, Mods::default(), &Fake);
        }
        e.key(Key::Enter, Mods::default(), &Fake);
        assert!(e.items.is_empty());
        e.undo(&Fake);
        assert_eq!(e.items[0].text, "Merhab\nğü!");
    }

    #[test]
    fn text_caret_navigation() {
        let mut t = TextEdit {
            item: None,
            pos: Point::default(),
            text: "ab\ncde".into(),
            caret: 6,
            before: None,
        };
        t.key(Key::Up, Mods::default());
        assert_eq!(t.caret, 2);
        t.key(Key::Home, Mods::default());
        assert_eq!(t.caret, 0);
        t.key(Key::Down, Mods::default());
        assert_eq!(t.caret, 3);
        t.key(Key::End, Mods::default());
        assert_eq!(t.caret, 6);
        t.key(Key::Left, Mods::default());
        t.key(Key::Delete, Mods::default());
        assert_eq!(t.text, "ab\ncd");
    }

    #[test]
    fn keyboard_commands() {
        let mut e = ed();
        assert_eq!(
            e.key(
                Key::Char('a'),
                Mods {
                    ctrl: true,
                    ..Mods::default()
                },
                &Fake
            ),
            Cmd::SelectMonitor
        );
        assert_eq!(
            e.key(Key::Char('c'), Mods::default(), &Fake),
            Cmd::CopyColor { hex: true }
        );
        e.key(Key::Shift, Mods::default(), &Fake);
        assert_eq!(
            e.key(Key::Char('c'), Mods::default(), &Fake),
            Cmd::CopyColor { hex: false }
        );
        assert_eq!(e.key(Key::Escape, Mods::default(), &Fake), Cmd::Close);
        let mut e = with_selection();
        assert_eq!(e.key(Key::Enter, Mods::default(), &Fake), Cmd::Copy);
        assert_eq!(
            e.key(
                Key::Char('s'),
                Mods {
                    ctrl: true,
                    ..Mods::default()
                },
                &Fake
            ),
            Cmd::Save
        );
        assert_eq!(e.key(Key::F3, Mods::default(), &Fake), Cmd::Pin);
        assert_eq!(
            e.key(
                Key::Char('p'),
                Mods {
                    ctrl: true,
                    ..Mods::default()
                },
                &Fake
            ),
            Cmd::Pin
        );
        assert_eq!(e.key(Key::Char('o'), Mods::default(), &Fake), Cmd::Ocr);
        e.key(Key::Char('r'), Mods::default(), &Fake);
        assert_eq!(e.key(Key::Escape, Mods::default(), &Fake), Cmd::None);
        assert_eq!(e.tool, Tool::Select);
        assert_eq!(e.key(Key::Escape, Mods::default(), &Fake), Cmd::Close);
    }

    #[test]
    fn arrow_keys_nudge_selection_or_item_and_stay_in_bounds() {
        let mut e = with_selection();
        e.key(
            Key::Right,
            Mods {
                shift: true,
                ..Mods::default()
            },
            &Fake,
        );
        assert_eq!(e.sel.unwrap().x, 110.0);
        e.key(Key::Up, Mods::default(), &Fake);
        assert_eq!(e.sel.unwrap().y, 99.0);
        let mut edge = ed();
        drag(&mut edge, (0.0, 0.0), (100.0, 100.0));
        edge.key(Key::Left, Mods::default(), &Fake);
        assert_eq!(
            edge.sel.unwrap(),
            Rect::new(0.0, 0.0, 100.0, 100.0),
            "no shrinking at the edge"
        );
        // item nudge is an undo step
        e.set_tool(Tool::Draw(Kind::Rect), &Fake);
        drag(&mut e, (150.0, 150.0), (300.0, 250.0));
        e.set_tool(Tool::Select, &Fake);
        e.press(Point::new(150.0, 200.0), Button::Left, &Fake);
        e.release();
        e.key(Key::Down, Mods::default(), &Fake);
        assert_eq!(e.items[0].first().y, 151.0);
        e.undo(&Fake);
        assert_eq!(e.items[0].first().y, 150.0);
    }

    #[test]
    fn right_click_steps_back_one_level_at_a_time() {
        let mut e = with_selection();
        e.set_tool(Tool::Draw(Kind::Rect), &Fake);
        assert_eq!(
            e.press(Point::new(0.0, 0.0), Button::Right, &Fake),
            Cmd::None
        );
        assert_eq!(e.tool, Tool::Select);
        drag(&mut e, (0.0, 0.0), (0.0, 0.0));
        e.press(Point::new(0.0, 0.0), Button::Right, &Fake);
        assert!(e.sel.is_none());
        assert_eq!(
            e.press(Point::new(0.0, 0.0), Button::Right, &Fake),
            Cmd::Close
        );
    }

    #[test]
    fn property_changes_apply_to_the_selected_item_but_are_not_undoable() {
        let mut e = with_selection();
        e.set_tool(Tool::Draw(Kind::Rect), &Fake);
        drag(&mut e, (150.0, 150.0), (300.0, 250.0));
        e.set_tool(Tool::Select, &Fake);
        e.press(Point::new(150.0, 200.0), Button::Left, &Fake);
        e.release();
        e.set_color(Rgba::rgb(0, 0, 255));
        e.set_width(9);
        e.set_fill(true);
        assert_eq!(
            (e.items[0].color, e.items[0].width, e.items[0].fill),
            (Rgba::rgb(0, 0, 255), 9, true)
        );
        e.undo(&Fake);
        assert!(
            e.items.is_empty(),
            "the undo step was the add, not the edits"
        );
    }

    #[test]
    fn double_click_on_empty_selection_copies_and_cursor_kinds() {
        let mut e = with_selection();
        assert_eq!(e.double_click(Point::new(300.0, 300.0), &Fake), Cmd::Copy);
        assert_eq!(e.double_click(Point::new(900.0, 700.0), &Fake), Cmd::None);
        assert_eq!(
            e.cursor_at(Point::new(300.0, 300.0), &Fake),
            CursorKind::Hand
        );
        assert_eq!(
            e.cursor_at(Point::new(900.0, 700.0), &Fake),
            CursorKind::Arrow
        );
        assert_eq!(
            e.cursor_at(Point::new(100.0, 100.0), &Fake),
            CursorKind::SizeFDiag
        );
        assert_eq!(
            e.cursor_at(Point::new(300.0, 100.0), &Fake),
            CursorKind::SizeVer
        );
        assert_eq!(
            ed().cursor_at(Point::new(1.0, 1.0), &Fake),
            CursorKind::Cross
        );
    }

    #[test]
    fn select_rect_only_fills_an_empty_selection() {
        let mut e = ed();
        e.select_rect(Rect::new(-50.0, 0.0, 1100.0, 800.0));
        assert_eq!(e.sel, Some(Rect::new(0.0, 0.0, 1000.0, 800.0)));
        e.select_rect(Rect::new(0.0, 0.0, 10.0, 10.0));
        assert_eq!(e.sel, Some(Rect::new(0.0, 0.0, 1000.0, 800.0)));
    }
}
