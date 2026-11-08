//! The built-in UI theme used until projects ship their own fonts and window skins (§4.2
//! `fonts/`, `ui/`): an original proportional 5×7 ASCII pixel font with descenders and a
//! framed window skin, both generated in code (no image files, no third-party licences).

use std::fmt::Write as _;

use pkrs_render::BitmapFont;
use pkrs_ui::{NineSliceSpec, WindowSkin};

use crate::ui_theme::UiTheme;

const CELL_W: u32 = 8;
const CELL_H: u32 = 10;
const COLUMNS: u32 = 16;
const FIRST: u32 = 0x20;
const LAST: u32 = 0x7e;
const LINE_HEIGHT: u32 = 10;
const BASE: u32 = 7;
const SPACE_ADVANCE: u32 = 4;

/// Glyph bitmaps: rows top to bottom separated by `/`, `#` set. Seven rows sit on the
/// baseline; rows 8–9 are descenders.
const GLYPHS: &[(char, &str)] = &[
    ('!', "..#../..#../..#../..#../..#../...../..#.."),
    ('"', ".#.#./.#.#./.#.#."),
    ('#', ".#.#./.#.#./#####/.#.#./#####/.#.#./.#.#."),
    ('$', "..#../.####/#.#../.###./..#.#/####./..#.."),
    ('%', "##.../##..#/...#./..#../.#.../#..##/...##"),
    ('&', ".##../#..#./#.#../.#.../#.#.#/#..#./.##.#"),
    ('\'', "..#../..#../..#.."),
    ('(', "...#./..#../.#.../.#.../.#.../..#../...#."),
    (')', ".#.../..#../...#./...#./...#./..#../.#..."),
    ('*', "...../..#../#.#.#/.###./#.#.#/..#.."),
    ('+', "...../..#../..#../#####/..#../..#.."),
    (',', "...../...../...../...../...../.##../..#../.#..."),
    ('-', "...../...../...../#####"),
    ('.', "...../...../...../...../...../.##../.##.."),
    ('/', "...../....#/...#./..#../.#.../#...."),
    ('0', ".###./#...#/#..##/#.#.#/##..#/#...#/.###."),
    ('1', "..#../.##../..#../..#../..#../..#../.###."),
    ('2', ".###./#...#/....#/...#./..#../.#.../#####"),
    ('3', "#####/...#./..#../...#./....#/#...#/.###."),
    ('4', "...#./..##./.#.#./#..#./#####/...#./...#."),
    ('5', "#####/#..../####./....#/....#/#...#/.###."),
    ('6', "..##./.#.../#..../####./#...#/#...#/.###."),
    ('7', "#####/....#/...#./..#../.#.../.#.../.#..."),
    ('8', ".###./#...#/#...#/.###./#...#/#...#/.###."),
    ('9', ".###./#...#/#...#/.####/....#/...#./.##.."),
    (':', "...../.##../.##../...../.##../.##.."),
    (';', "...../.##../.##../...../.##../..#../.#..."),
    ('<', "...#./..#../.#.../#..../.#.../..#../...#."),
    ('=', "...../...../#####/...../#####"),
    ('>', ".#.../..#../...#./....#/...#./..#../.#..."),
    ('?', ".###./#...#/....#/...#./..#../...../..#.."),
    ('@', ".###./#...#/....#/.##.#/#.#.#/#.#.#/.###."),
    ('A', ".###./#...#/#...#/#####/#...#/#...#/#...#"),
    ('B', "####./#...#/#...#/####./#...#/#...#/####."),
    ('C', ".###./#...#/#..../#..../#..../#...#/.###."),
    ('D', "####./#...#/#...#/#...#/#...#/#...#/####."),
    ('E', "#####/#..../#..../####./#..../#..../#####"),
    ('F', "#####/#..../#..../####./#..../#..../#...."),
    ('G', ".###./#...#/#..../#.###/#...#/#...#/.####"),
    ('H', "#...#/#...#/#...#/#####/#...#/#...#/#...#"),
    ('I', "###/.#./.#./.#./.#./.#./###"),
    ('J', "..###/...#./...#./...#./...#./#..#./.##.."),
    ('K', "#...#/#..#./#.#../##.../#.#../#..#./#...#"),
    ('L', "#..../#..../#..../#..../#..../#..../#####"),
    ('M', "#...#/##.##/#.#.#/#.#.#/#...#/#...#/#...#"),
    ('N', "#...#/#...#/##..#/#.#.#/#..##/#...#/#...#"),
    ('O', ".###./#...#/#...#/#...#/#...#/#...#/.###."),
    ('P', "####./#...#/#...#/####./#..../#..../#...."),
    ('Q', ".###./#...#/#...#/#...#/#.#.#/#..#./.##.#"),
    ('R', "####./#...#/#...#/####./#.#../#..#./#...#"),
    ('S', ".####/#..../#..../.###./....#/....#/####."),
    ('T', "#####/..#../..#../..#../..#../..#../..#.."),
    ('U', "#...#/#...#/#...#/#...#/#...#/#...#/.###."),
    ('V', "#...#/#...#/#...#/#...#/#...#/.#.#./..#.."),
    ('W', "#...#/#...#/#...#/#.#.#/#.#.#/#.#.#/.#.#."),
    ('X', "#...#/#...#/.#.#./..#../.#.#./#...#/#...#"),
    ('Y', "#...#/#...#/.#.#./..#../..#../..#../..#.."),
    ('Z', "#####/....#/...#./..#../.#.../#..../#####"),
    ('[', "###/#../#../#../#../#../###"),
    ('\\', "...../#..../.#.../..#../...#./....#"),
    (']', "###/..#/..#/..#/..#/..#/###"),
    ('^', "..#../.#.#./#...#"),
    ('_', "...../...../...../...../...../...../#####"),
    ('`', "#../.#."),
    ('a', "...../...../.###./....#/.####/#...#/.####"),
    ('b', "#..../#..../#.##./##..#/#...#/#...#/####."),
    ('c', "...../...../.###./#..../#..../#...#/.###."),
    ('d', "....#/....#/.##.#/#..##/#...#/#...#/.####"),
    ('e', "...../...../.###./#...#/#####/#..../.###."),
    ('f', "..##/.#../.#../###./.#../.#../.#.."),
    ('g', "...../...../.####/#...#/#...#/.####/....#/....#/.###."),
    ('h', "#..../#..../#.##./##..#/#...#/#...#/#...#"),
    ('i', ".#./.../##./.#./.#./.#./###"),
    ('j', "..#/.../.##/..#/..#/..#/..#/#.#/.#."),
    ('k', "#.../#.../#..#/#.#./##../#.#./#..#"),
    ('l', "##./.#./.#./.#./.#./.#./###"),
    ('m', "...../...../##.#./#.#.#/#.#.#/#...#/#...#"),
    ('n', "...../...../#.##./##..#/#...#/#...#/#...#"),
    ('o', "...../...../.###./#...#/#...#/#...#/.###."),
    ('p', "...../...../####./#...#/#...#/####./#..../#..../#...."),
    ('q', "...../...../.####/#...#/#...#/.####/....#/....#/....#"),
    ('r', "...../...../#.##./##..#/#..../#..../#...."),
    ('s', "...../...../.####/#..../.###./....#/####."),
    ('t', ".#../.#../###./.#../.#../.#.#/..#."),
    ('u', "...../...../#...#/#...#/#...#/#..##/.##.#"),
    ('v', "...../...../#...#/#...#/#...#/.#.#./..#.."),
    ('w', "...../...../#...#/#...#/#.#.#/#.#.#/.#.#."),
    ('x', "...../...../#...#/.#.#./..#../.#.#./#...#"),
    ('y', "...../...../#...#/#...#/#...#/.####/....#/....#/.###."),
    ('z', "...../...../#####/...#./..#../.#.../#####"),
    ('{', "..#/.#./.#./#../.#./.#./..#"),
    ('|', "#/#/#/#/#/#/#"),
    ('}', "#../.#./.#./..#/.#./.#./#.."),
    ('~', "...../...../.#.../#.#.#/...#."),
];

/// Builds the default font and window skin on `renderer`.
pub fn build_default_ui_theme(
    renderer: &mut pkrs_render::Renderer,
) -> Result<UiTheme, pkrs_ui::UiError> {
    let (atlas, ron) = font_atlas();
    let font: BitmapFont = pkrs_render::parse_glyph_map_ron(&ron)?;
    let font_tex = renderer.create_texture(font.atlas_width, font.atlas_height, &atlas)?;
    let skin_tex = renderer.create_texture(SKIN_SIZE, SKIN_SIZE, &skin_atlas())?;
    let skin = WindowSkin::new(
        skin_tex,
        SKIN_SIZE,
        SKIN_SIZE,
        NineSliceSpec {
            border: SKIN_BORDER,
        },
    )?;
    Ok(UiTheme::new(font, font_tex, skin))
}

fn cell_origin(codepoint: u32) -> (u32, u32) {
    let index = codepoint - FIRST;
    ((index % COLUMNS) * CELL_W, (index / COLUMNS) * CELL_H)
}

/// The white-on-transparent RGBA atlas and its RON glyph map.
fn font_atlas() -> (Vec<u8>, String) {
    let rows = (LAST - FIRST) / COLUMNS + 1;
    let (aw, ah) = (COLUMNS * CELL_W, rows * CELL_H);
    let mut rgba = vec![0u8; (aw * ah * 4) as usize];
    let mut set = |x: u32, y: u32| {
        let i = ((y * aw + x) * 4) as usize;
        rgba[i..i + 4].copy_from_slice(&[255; 4]);
    };
    // Dialogue "more" blips and fallback panels sample texel (0, 0) (`pkrs_ui::draw_more_arrow`).
    set(0, 0);
    let mut ron = format!(
        "(line_height: {LINE_HEIGHT}, base: {BASE}, atlas: (width: {aw}, height: {ah}), glyphs: {{\n"
    );
    // Space: an empty texel away from (0, 0).
    let _ = writeln!(
        ron,
        "\"U+0020\": (x: 2, y: 2, w: 1, h: 1, xadv: {SPACE_ADVANCE}),"
    );
    for &(ch, bitmap) in GLYPHS {
        let (ox, oy) = cell_origin(u32::from(ch));
        let mut width = 0;
        let mut height = 0;
        for (y, row) in bitmap.split('/').enumerate() {
            height = y as u32 + 1;
            width = width.max(row.len() as u32);
            for (x, px) in row.bytes().enumerate() {
                if px == b'#' {
                    set(ox + x as u32, oy + y as u32);
                }
            }
        }
        let _ = writeln!(
            ron,
            "\"U+{:04X}\": (x: {ox}, y: {oy}, w: {width}, h: {height}, xadv: {}),",
            u32::from(ch),
            width + 1
        );
    }
    ron.push_str("})");
    (rgba, ron)
}

const SKIN_SIZE: u32 = 12;
const SKIN_BORDER: u32 = 4;

/// Dark outline, light frame, dark inner line, navy fill; the corner texels are cut.
fn skin_atlas() -> Vec<u8> {
    const OUTLINE: [u8; 4] = [16, 24, 40, 255];
    const FRAME: [u8; 4] = [224, 232, 248, 255];
    const FILL: [u8; 4] = [40, 56, 104, 255];
    let last = SKIN_SIZE - 1;
    let mut rgba = Vec::with_capacity((SKIN_SIZE * SKIN_SIZE * 4) as usize);
    for y in 0..SKIN_SIZE {
        for x in 0..SKIN_SIZE {
            let corner = (x == 0 || x == last) && (y == 0 || y == last);
            let edge = x.min(y).min(last - x).min(last - y);
            let px = match edge {
                _ if corner => [0; 4],
                0 | 3 => OUTLINE,
                1 | 2 => FRAME,
                _ => FILL,
            };
            rgba.extend_from_slice(&px);
        }
    }
    rgba
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn every_printable_ascii_character_has_a_glyph() {
        let (_, ron) = font_atlas();
        let font = pkrs_render::parse_glyph_map_ron(&ron).unwrap();
        for cp in FIRST..=LAST {
            let ch = char::from_u32(cp).unwrap();
            assert!(font.glyph(cp).is_some(), "missing `{ch}`");
        }
        assert_eq!(GLYPHS.len() as u32, LAST - FIRST);
    }

    #[test]
    fn glyphs_fit_their_cells() {
        for &(ch, bitmap) in GLYPHS {
            let rows: Vec<&str> = bitmap.split('/').collect();
            assert!((rows.len() as u32) < CELL_H, "`{ch}` is too tall");
            let width = rows[0].len();
            for row in &rows {
                assert_eq!(row.len(), width, "`{ch}` has ragged rows");
                assert!(row.bytes().all(|b| b == b'#' || b == b'.'), "`{ch}`");
            }
            assert!((width as u32) < CELL_W, "`{ch}` is too wide");
            assert!(bitmap.contains('#'), "`{ch}` is blank");
        }
    }

    #[test]
    fn the_more_arrow_texel_is_opaque_white() {
        let (rgba, _) = font_atlas();
        assert_eq!(&rgba[..4], &[255; 4]);
    }

    #[test]
    fn skin_is_framed_and_filled() {
        let rgba = skin_atlas();
        let px = |x: u32, y: u32| {
            let i = ((y * SKIN_SIZE + x) * 4) as usize;
            [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
        };
        assert_eq!(px(0, 0)[3], 0, "rounded corner");
        assert_eq!(px(1, 5), [224, 232, 248, 255]);
        assert_eq!(px(6, 6), [40, 56, 104, 255]);
    }
}
