//! A 16x16 canvas with a 3x5 font and small widgets, for text on the display with no font library.
//!
//! The font and the widget layouts come from ih8d8/divoom-ditoo-controller (MIT license, see `third_party/`).

use image::{DynamicImage, Rgb, RgbImage};

pub const SIZE: usize = 16;
pub type Color = [u8; 3];
pub const WHITE: Color = [240, 240, 240];
pub const GREY: Color = [160, 160, 160];
pub const DIM: Color = [80, 80, 80];
pub const RED: Color = [239, 68, 68];
pub const AMBER: Color = [245, 158, 11];
pub const GREEN: Color = [34, 197, 94];
pub const CYAN: Color = [0, 229, 255];

/// Glyph width and its five rows. The highest used bit is the leftmost pixel.
fn glyph(c: char) -> (usize, [u8; 5]) {
  match c.to_ascii_uppercase() {
    '0' | 'O' => (3, [0b111, 0b101, 0b101, 0b101, 0b111]),
    '1' => (1, [0b1, 0b1, 0b1, 0b1, 0b1]),
    '2' => (3, [0b111, 0b001, 0b111, 0b100, 0b111]),
    '3' => (3, [0b111, 0b001, 0b111, 0b001, 0b111]),
    '4' => (3, [0b101, 0b101, 0b111, 0b001, 0b001]),
    '5' => (3, [0b111, 0b100, 0b111, 0b001, 0b111]),
    '6' => (3, [0b111, 0b100, 0b111, 0b101, 0b111]),
    '7' => (3, [0b111, 0b001, 0b010, 0b010, 0b010]),
    '8' => (3, [0b111, 0b101, 0b111, 0b101, 0b111]),
    '9' => (3, [0b111, 0b101, 0b111, 0b001, 0b111]),
    '%' => (3, [0b101, 0b001, 0b010, 0b100, 0b101]),
    'A' => (3, [0b010, 0b101, 0b111, 0b101, 0b101]),
    'B' => (3, [0b110, 0b101, 0b110, 0b101, 0b110]),
    'C' => (3, [0b111, 0b100, 0b100, 0b100, 0b111]),
    'D' => (3, [0b110, 0b101, 0b101, 0b101, 0b110]),
    'E' => (3, [0b111, 0b100, 0b110, 0b100, 0b111]),
    'F' => (3, [0b111, 0b100, 0b110, 0b100, 0b100]),
    'G' => (3, [0b111, 0b100, 0b101, 0b101, 0b111]),
    'H' => (3, [0b101, 0b101, 0b111, 0b101, 0b101]),
    'I' => (3, [0b111, 0b010, 0b010, 0b010, 0b111]),
    'J' => (3, [0b001, 0b001, 0b001, 0b101, 0b010]),
    'K' => (3, [0b101, 0b110, 0b100, 0b110, 0b101]),
    'L' => (3, [0b100, 0b100, 0b100, 0b100, 0b111]),
    'M' => (5, [0b10001, 0b11011, 0b10101, 0b10001, 0b10001]),
    'N' => (3, [0b101, 0b111, 0b111, 0b101, 0b101]),
    'P' => (3, [0b111, 0b101, 0b111, 0b100, 0b100]),
    'Q' => (3, [0b111, 0b101, 0b101, 0b111, 0b001]),
    'R' => (3, [0b110, 0b101, 0b110, 0b101, 0b101]),
    'S' => (3, [0b011, 0b100, 0b010, 0b001, 0b110]),
    'T' => (3, [0b111, 0b010, 0b010, 0b010, 0b010]),
    'U' => (3, [0b101, 0b101, 0b101, 0b101, 0b111]),
    'V' => (3, [0b101, 0b101, 0b101, 0b101, 0b010]),
    'W' => (5, [0b10001, 0b10001, 0b10101, 0b11011, 0b10001]),
    'X' => (3, [0b101, 0b101, 0b010, 0b101, 0b101]),
    'Y' => (3, [0b101, 0b101, 0b010, 0b010, 0b010]),
    'Z' => (3, [0b111, 0b001, 0b010, 0b100, 0b111]),
    ' ' => (2, [0, 0, 0, 0, 0]),
    '!' => (1, [0b1, 0b1, 0b1, 0b0, 0b1]),
    ':' => (1, [0b0, 0b1, 0b0, 0b1, 0b0]),
    '.' => (1, [0b0, 0b0, 0b0, 0b0, 0b1]),
    '-' => (3, [0b000, 0b000, 0b111, 0b000, 0b000]),
    '+' => (3, [0b010, 0b111, 0b010, 0b000, 0b000]),
    '/' => (3, [0b001, 0b001, 0b010, 0b100, 0b100]),
    '?' => (3, [0b110, 0b001, 0b010, 0b000, 0b010]),
    _ => (3, [0b111, 0b101, 0b101, 0b101, 0b111]),
  }
}

/// Width of the text in pixels, with one pixel between glyphs.
pub fn text_width(text: &str) -> usize {
  let count = text.chars().count();
  text.chars().map(|c| glyph(c).0).sum::<usize>() + count.saturating_sub(1)
}

#[derive(Clone)]
pub struct Canvas {
  pixels: [Color; SIZE * SIZE],
}

impl Canvas {
  pub fn new(background: Color) -> Self {
    Self { pixels: [background; SIZE * SIZE] }
  }

  pub fn set(&mut self, x: usize, y: usize, color: Color) {
    if x < SIZE && y < SIZE {
      self.pixels[y * SIZE + x] = color;
    }
  }

  pub fn get(&self, x: usize, y: usize) -> Color {
    self.pixels[y * SIZE + x]
  }

  pub fn fill_rect(&mut self, x: usize, y: usize, w: usize, h: usize, color: Color) {
    for yy in y..y + h {
      for xx in x..x + w {
        self.set(xx, yy, color);
      }
    }
  }

  pub fn rect(&mut self, x: usize, y: usize, w: usize, h: usize, color: Color) {
    for xx in x..x + w {
      self.set(xx, y, color);
      self.set(xx, y + h - 1, color);
    }
    for yy in y..y + h {
      self.set(x, yy, color);
      self.set(x + w - 1, yy, color);
    }
  }

  pub fn text(&mut self, mut x: usize, y: usize, text: &str, color: Color) {
    for c in text.chars() {
      let (width, rows) = glyph(c);
      for (row, bits) in rows.iter().enumerate() {
        for col in 0..width {
          if (bits >> (width - 1 - col)) & 1 == 1 {
            self.set(x + col, y + row, color);
          }
        }
      }
      x += width + 1;
    }
  }

  pub fn text_centered(&mut self, y: usize, text: &str, color: Color) {
    self.text(SIZE.saturating_sub(text_width(text)) / 2, y, text, color);
  }

  pub fn to_image(&self) -> DynamicImage {
    let mut image = RgbImage::new(SIZE as u32, SIZE as u32);
    for (i, color) in self.pixels.iter().enumerate() {
      image.put_pixel((i % SIZE) as u32, (i / SIZE) as u32, Rgb(*color));
    }
    DynamicImage::ImageRgb8(image)
  }
}

/// Two lines of text, each up to 4 letters wide.
pub fn card(top: &str, bottom: &str, top_color: Color, bottom_color: Color, background: Color) -> Canvas {
  let mut canvas = Canvas::new(background);
  canvas.text_centered(2, top, top_color);
  canvas.text_centered(9, bottom, bottom_color);
  canvas
}

/// A label above a progress bar.
pub fn gauge(label: &str, value: u32, maximum: u32, color: Color) -> Canvas {
  let mut canvas = Canvas::new([0, 0, 0]);
  canvas.text_centered(2, label, WHITE);
  canvas.rect(1, 9, 14, 5, DIM);
  let fill = if maximum == 0 { 0 } else { (value as usize * 12 / maximum as usize).min(12) };
  canvas.fill_rect(2, 10, fill, 3, color);
  canvas
}

/// A small grey label above a value.
pub fn metric(label: &str, value: &str, color: Color) -> Canvas {
  let mut canvas = Canvas::new([0, 0, 0]);
  canvas.text_centered(2, label, GREY);
  canvas.text_centered(9, value, color);
  canvas
}

/// A battery outline with a fill bar and the percentage below it.
pub fn battery(percent: u8, charging: bool) -> Canvas {
  let mut canvas = Canvas::new([0, 0, 0]);
  let percent = percent.min(100);
  let low = percent <= 20;
  let bar = if charging { CYAN } else if low { RED } else if percent <= 40 { AMBER } else { GREEN };
  let frame = if low { [220, 38, 38] } else { [120, 120, 120] };
  canvas.rect(1, 1, 13, 6, frame);
  for y in 2..=4 {
    canvas.set(14, y, frame);
  }
  let fill = if percent == 0 { 0 } else { (percent as usize * 11 + 50) / 100 };
  canvas.fill_rect(2, 2, fill, 4, bar);
  if charging {
    for (x, y) in [(7, 2), (6, 3), (7, 3), (8, 3), (7, 4), (6, 5)] {
      canvas.set(x, y, [255, 235, 59]);
    }
  }
  let text_color = if low { RED } else if charging { CYAN } else { WHITE };
  canvas.text_centered(9, &format!("{percent}%"), text_color);
  canvas
}

#[cfg(test)]
mod tests {
  use super::*;

  fn rows(canvas: &Canvas) -> Vec<String> {
    (0..SIZE)
      .map(|y| (0..SIZE).map(|x| if canvas.get(x, y) == [0, 0, 0] { '.' } else { '#' }).collect())
      .collect()
  }

  #[test]
  fn card_draws_two_centered_lines() {
    let canvas = card("DONE", "TEST", GREEN, WHITE, [0, 0, 0]);
    let rows = rows(&canvas);
    assert_eq!(rows[2], "##..###.#.#.###.");
    assert_eq!(rows[6], "##..###.#.#.###.");
    assert_eq!(rows[9], "###.###..##.###.");
    assert_eq!(rows[13], ".#..###.##...#..");
    assert_eq!(rows[0], "................");
  }

  #[test]
  fn text_width_counts_gaps() {
    assert_eq!(text_width("100%"), 1 + 1 + 3 + 1 + 3 + 1 + 3);
    assert_eq!(text_width(""), 0);
  }

  #[test]
  fn gauge_fill_is_proportional_and_clamped() {
    let lit = |c: &Canvas| (2..14).filter(|&x| c.get(x, 11) == GREEN).count();
    assert_eq!(lit(&gauge("CPU", 0, 100, GREEN)), 0);
    assert_eq!(lit(&gauge("CPU", 50, 100, GREEN)), 6);
    assert_eq!(lit(&gauge("CPU", 500, 100, GREEN)), 12);
    assert_eq!(lit(&gauge("CPU", 5, 0, GREEN)), 0);
  }
}
