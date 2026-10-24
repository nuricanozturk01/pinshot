//! Development helper: drive the X11 desktop with synthetic input and take screenshots.
//!
//! usage: xdrive <command> [args] ...   (commands run left to right)
//!   move X Y | click [1|3] | down | up | drag X1 Y1 X2 Y2 | dclick | key NAME [ctrl|shift|alt]... | type TEXT
//!   sleep MS | shot FILE.png | shotrect FILE.png X Y W H

use std::time::Duration;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt, ImageFormat};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;

struct X {
    c: RustConnection,
    root: u32,
    min: u8,
    per: u8,
    map: Vec<u32>,
}

impl X {
    fn new() -> X {
        let (c, n) = x11rb::connect(None).expect("X11");
        let root = c.setup().roots[n].root;
        let (min, max) = (c.setup().min_keycode, c.setup().max_keycode);
        let m = c
            .get_keyboard_mapping(min, max - min + 1)
            .unwrap()
            .reply()
            .unwrap();
        X {
            c,
            root,
            min,
            per: m.keysyms_per_keycode,
            map: m.keysyms,
        }
    }

    fn code(&self, keysym: u32) -> Option<(u8, bool)> {
        for (i, chunk) in self.map.chunks(self.per as usize).enumerate() {
            if let Some(col) = chunk.iter().position(|&k| k == keysym) {
                return Some((self.min + i as u8, col % 2 == 1 && col < 2));
            }
        }
        None
    }

    fn fake(&self, ty: u8, detail: u8, x: i16, y: i16) {
        self.c
            .xtest_fake_input(ty, detail, 0, self.root, x, y, 0)
            .unwrap();
        self.c.flush().unwrap();
    }

    fn key(&self, keysym: u32, down: bool) {
        if let Some((code, _)) = self.code(keysym) {
            self.fake(if down { 2 } else { 3 }, code, 0, 0);
        } else {
            eprintln!("no keycode for keysym {keysym:#x}");
        }
    }

    fn pos(&self) -> (i16, i16) {
        let r = self.c.query_pointer(self.root).unwrap().reply().unwrap();
        (r.root_x, r.root_y)
    }

    fn mv(&self, x: i16, y: i16) {
        self.fake(6, 0, x, y);
    }

    fn button(&self, b: u8, down: bool) {
        self.fake(if down { 4 } else { 5 }, b, 0, 0);
    }

    fn shot(&self, file: &str, rect: Option<(i16, i16, u16, u16)>) {
        let g = self.c.get_geometry(self.root).unwrap().reply().unwrap();
        let (x, y, w, h) = rect.unwrap_or((0, 0, g.width, g.height));
        let img = self
            .c
            .get_image(ImageFormat::Z_PIXMAP, self.root, x, y, w, h, !0)
            .unwrap()
            .reply()
            .unwrap();
        let mut out = image::RgbImage::new(w as u32, h as u32);
        for (i, p) in img.data.chunks_exact(4).enumerate() {
            out.put_pixel(
                i as u32 % w as u32,
                i as u32 / w as u32,
                image::Rgb([p[2], p[1], p[0]]),
            );
        }
        out.save(file).unwrap();
    }
}

fn keysym_for(name: &str) -> u32 {
    match name {
        "Return" | "Enter" => 0xff0d,
        "Escape" | "Esc" => 0xff1b,
        "BackSpace" | "Backspace" => 0xff08,
        "Delete" | "Del" => 0xffff,
        "Left" => 0xff51,
        "Up" => 0xff52,
        "Right" => 0xff53,
        "Down" => 0xff54,
        "Home" => 0xff50,
        "End" => 0xff57,
        "F3" => 0xffc0,
        "space" | "Space" => 0x20,
        n if n.chars().count() == 1 => n.chars().next().unwrap() as u32,
        other => panic!("unknown key {other}"),
    }
}

fn main() {
    let x = X::new();
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    let num = |s: &String| s.parse::<i16>().unwrap();
    while i < a.len() {
        match a[i].as_str() {
            "move" => {
                x.mv(num(&a[i + 1]), num(&a[i + 2]));
                i += 3;
            }
            "click" => {
                let b = a.get(i + 1).and_then(|s| s.parse::<u8>().ok());
                x.button(b.unwrap_or(1), true);
                std::thread::sleep(Duration::from_millis(40));
                x.button(b.unwrap_or(1), false);
                i += if b.is_some() { 2 } else { 1 };
            }
            "dclick" => {
                for _ in 0..2 {
                    x.button(1, true);
                    std::thread::sleep(Duration::from_millis(30));
                    x.button(1, false);
                    std::thread::sleep(Duration::from_millis(60));
                }
                i += 1;
            }
            "down" => {
                x.button(1, true);
                i += 1;
            }
            "up" => {
                x.button(1, false);
                i += 1;
            }
            "drag" => {
                let (x1, y1, x2, y2) = (
                    num(&a[i + 1]),
                    num(&a[i + 2]),
                    num(&a[i + 3]),
                    num(&a[i + 4]),
                );
                x.mv(x1, y1);
                std::thread::sleep(Duration::from_millis(80));
                x.button(1, true);
                for s in 1..=12 {
                    std::thread::sleep(Duration::from_millis(25));
                    x.mv(x1 + (x2 - x1) * s / 12, y1 + (y2 - y1) * s / 12);
                }
                std::thread::sleep(Duration::from_millis(60));
                x.button(1, false);
                i += 5;
            }
            "key" => {
                let name = &a[i + 1];
                let mut j = i + 2;
                let mut held = vec![];
                while j < a.len() && matches!(a[j].as_str(), "ctrl" | "shift" | "alt") {
                    let ks = match a[j].as_str() {
                        "ctrl" => 0xffe3,
                        "shift" => 0xffe1,
                        _ => 0xffe9,
                    };
                    x.key(ks, true);
                    held.push(ks);
                    j += 1;
                }
                x.key(keysym_for(name), true);
                std::thread::sleep(Duration::from_millis(30));
                x.key(keysym_for(name), false);
                for ks in held.into_iter().rev() {
                    x.key(ks, false);
                }
                i = j;
            }
            "type" => {
                for ch in a[i + 1].chars() {
                    let upper = ch.is_uppercase();
                    if upper {
                        x.key(0xffe1, true);
                    }
                    let ks = ch.to_lowercase().next().unwrap() as u32;
                    x.key(ks, true);
                    std::thread::sleep(Duration::from_millis(15));
                    x.key(ks, false);
                    if upper {
                        x.key(0xffe1, false);
                    }
                }
                i += 2;
            }
            "sleep" => {
                std::thread::sleep(Duration::from_millis(a[i + 1].parse().unwrap()));
                i += 2;
            }
            "shot" => {
                x.shot(&a[i + 1], None);
                i += 2;
            }
            "shotrect" => {
                x.shot(
                    &a[i + 1],
                    Some((
                        num(&a[i + 2]),
                        num(&a[i + 3]),
                        a[i + 4].parse().unwrap(),
                        a[i + 5].parse().unwrap(),
                    )),
                );
                i += 6;
            }
            "pos" => {
                println!("{:?}", x.pos());
                i += 1;
            }
            other => panic!("unknown command {other}"),
        }
    }
}
