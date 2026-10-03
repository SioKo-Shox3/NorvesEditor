//! 時刻見出し付き画像一覧を、上限内で生成する。

use std::io::{self, Cursor, Write};

use image::{
    codecs::png::PngEncoder, DynamicImage, ExtendedColorType, ImageEncoder, ImageFormat,
    ImageReader, Limits, Rgba, RgbaImage,
};

use crate::error::BackendError;

const MAX_IMAGE_COUNT: usize = 16;
const MAX_INPUT_DIMENSION: u32 = 4096;
const MAX_INPUT_PIXELS: u64 = 16_777_216;
const MAX_INPUT_PNG_BYTES: usize = 2 * 1024 * 1024;
const MAX_OUTPUT_PNG_BYTES: usize = 2 * 1024 * 1024;
const OUTPUT_PNG_BUFFER_RESERVATION_BYTES: usize = MAX_OUTPUT_PNG_BYTES * 2;
const MAX_WORKING_BYTES: usize = 128 * 1024 * 1024;
const MAX_OUTPUT_LONG_EDGE: u32 = 2048;
const MAX_DECODED_RASTER_BYTES: usize = 64 * 1024 * 1024;
const MAX_RESIZED_RASTER_BYTES: usize = 2 * 1024 * 1024;
const PNG_DECODER_OVERHEAD_BYTES: usize = 8 * 1024 * 1024;

const COLUMNS: usize = 4;
const CELL_WIDTH: u32 = 500;
const CELL_HEIGHT: u32 = 500;
const CELL_GAP: u32 = 8;
const OUTER_MARGIN: u32 = 12;
const HEADER_HEIGHT: u32 = 32;
const CONTENT_INSET: u32 = 6;
const CONTENT_WIDTH: u32 = CELL_WIDTH - CONTENT_INSET * 2;
const CONTENT_HEIGHT: u32 = CELL_HEIGHT - HEADER_HEIGHT - CONTENT_INSET * 2;
const GLYPH_WIDTH: u32 = 5;
const GLYPH_HEIGHT: u32 = 7;
const GLYPH_SPACING: u32 = 1;

const SHEET_COLOR: Rgba<u8> = Rgba([37, 42, 50, 255]);
const HEADER_COLOR: Rgba<u8> = Rgba([20, 24, 31, 255]);
const TEXT_COLOR: Rgba<u8> = Rgba([245, 245, 245, 255]);

/// 入力順に並べるPNG画像と、各画像に付ける経過時刻。
pub(crate) struct TimestampedImage<'a> {
    pub(crate) timestamp: &'a str,
    pub(crate) png: &'a [u8],
}

/// 生成した時刻付き一覧と、同じ順番・時刻を示すテキスト。
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct TimestampedImageList {
    pub(crate) png: Vec<u8>,
    pub(crate) text: String,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

#[derive(Clone, Copy)]
struct PngInfo {
    width: u32,
    height: u32,
    bit_depth: u8,
    color_type: u8,
}

#[derive(Clone, Copy)]
struct SheetLayout {
    columns: u32,
    rows: u32,
    width: u32,
    height: u32,
}

/// 検査済みPNGを入力順に配置し、見出しと説明文を付けて返す。
pub(crate) fn build_timestamped_image_list(
    frames: &[TimestampedImage<'_>],
) -> Result<TimestampedImageList, BackendError> {
    if frames.is_empty() {
        return Err(image_error("画像一覧には1枚以上の画像が必要です。"));
    }
    if frames.len() > MAX_IMAGE_COUNT {
        return Err(image_error("画像一覧は最大16枚です。"));
    }

    let layout = sheet_layout(frames.len())?;
    let canvas_bytes = checked_rgba_bytes(layout.width, layout.height)?;
    let mut input_bytes = 0usize;
    let mut largest_decoded_image = 0usize;
    let mut infos = Vec::with_capacity(frames.len());

    for frame in frames {
        validate_timestamp(frame.timestamp)?;
        if frame.png.is_empty() {
            return Err(image_error("入力PNGが空です。"));
        }
        if frame.png.len() > MAX_INPUT_PNG_BYTES {
            return Err(image_error("入力PNGが2 MiBの上限を超えています。"));
        }
        input_bytes = input_bytes
            .checked_add(frame.png.len())
            .ok_or_else(|| image_error("画像一覧の入力サイズ計算でoverflowしました。"))?;

        let info = png_info(frame.png)?;
        let (_, decoded_bytes) = validate_input_dimensions(info)?;
        validate_png_pixel_format(info)?;
        largest_decoded_image = largest_decoded_image.max(decoded_bytes);
        infos.push(info);
    }

    validate_working_budget(input_bytes, largest_decoded_image, canvas_bytes)?;

    let mut sheet = RgbaImage::from_pixel(layout.width, layout.height, SHEET_COLOR);
    let mut text = String::from("画像一覧（入力順）");
    for (index, (frame, info)) in frames.iter().zip(infos).enumerate() {
        let (cell_x, cell_y) = cell_origin(layout, index)?;
        draw_header(&mut sheet, cell_x, cell_y, index + 1, frame.timestamp);
        let decoded = decode_png(frame.png, info)?;
        let (width, height) =
            fit_dimensions(info.width, info.height, CONTENT_WIDTH, CONTENT_HEIGHT)?;
        let resized = if (width, height) == (info.width, info.height) {
            decoded
        } else {
            decoded.thumbnail_exact(width, height)
        };
        let rgba = resized.into_rgba8();
        let image_x = cell_x
            .checked_add(CONTENT_INSET)
            .and_then(|x| x.checked_add((CONTENT_WIDTH - rgba.width()) / 2))
            .ok_or_else(|| image_error("画像位置の計算でoverflowしました。"))?;
        let image_y = cell_y
            .checked_add(HEADER_HEIGHT)
            .and_then(|y| y.checked_add(CONTENT_INSET))
            .and_then(|y| y.checked_add((CONTENT_HEIGHT - rgba.height()) / 2))
            .ok_or_else(|| image_error("画像位置の計算でoverflowしました。"))?;
        image::imageops::replace(&mut sheet, &rgba, i64::from(image_x), i64::from(image_y));
        text.push_str(&format!("\n{:02}: {}", index + 1, frame.timestamp));
    }

    let mut output = BoundedPngWriter::default();
    PngEncoder::new(&mut output)
        .write_image(
            sheet.as_raw(),
            layout.width,
            layout.height,
            ExtendedColorType::Rgba8,
        )
        .map_err(|_| {
            if output.exceeded_limit {
                image_error("出力PNGが2 MiBの上限を超えています。")
            } else {
                image_error("画像一覧PNGを符号化できません。")
            }
        })?;
    Ok(TimestampedImageList {
        png: output.bytes,
        text,
        width: layout.width,
        height: layout.height,
    })
}

fn validate_timestamp(timestamp: &str) -> Result<(), BackendError> {
    let bytes = timestamp.as_bytes();
    if bytes.len() != 12
        || bytes[2] != b':'
        || bytes[5] != b':'
        || bytes[8] != b'.'
        || !bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 2 | 5 | 8) || byte.is_ascii_digit())
    {
        return Err(image_error("時刻はHH:MM:SS.mmm形式で指定してください。"));
    }
    let minute = two_digit_value(&bytes[3..5]);
    let second = two_digit_value(&bytes[6..8]);
    if minute >= 60 || second >= 60 {
        return Err(image_error(
            "時刻の分と秒は00から59の範囲で指定してください。",
        ));
    }
    Ok(())
}

fn two_digit_value(bytes: &[u8]) -> u8 {
    (bytes[0] - b'0') * 10 + (bytes[1] - b'0')
}

fn png_info(bytes: &[u8]) -> Result<PngInfo, BackendError> {
    const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if bytes.len() < 26
        || &bytes[..8] != PNG_SIGNATURE
        || bytes[8..12] != 13u32.to_be_bytes()
        || &bytes[12..16] != b"IHDR"
    {
        return Err(image_error("入力PNGの署名またはIHDRが不正です。"));
    }
    let width = u32::from_be_bytes(
        bytes[16..20]
            .try_into()
            .map_err(|_| image_error("PNGの幅を読み取れません。"))?,
    );
    let height = u32::from_be_bytes(
        bytes[20..24]
            .try_into()
            .map_err(|_| image_error("PNGの高さを読み取れません。"))?,
    );
    Ok(PngInfo {
        width,
        height,
        bit_depth: bytes[24],
        color_type: bytes[25],
    })
}

fn validate_png_pixel_format(info: PngInfo) -> Result<(), BackendError> {
    let supported = match info.color_type {
        0 | 3 => matches!(info.bit_depth, 1 | 2 | 4 | 8),
        2 | 4 | 6 => info.bit_depth == 8,
        _ => false,
    };
    if supported {
        Ok(())
    } else {
        Err(image_error(
            "入力PNGは8 bit以下のグレースケール、パレット、RGB形式にしてください。",
        ))
    }
}

fn validate_input_dimensions(info: PngInfo) -> Result<(u64, usize), BackendError> {
    let pixels = checked_pixel_count(info.width, info.height)?;
    let decoded_bytes = checked_rgba_bytes(info.width, info.height)?;
    if info.width == 0
        || info.height == 0
        || info.width > MAX_INPUT_DIMENSION
        || info.height > MAX_INPUT_DIMENSION
        || pixels > MAX_INPUT_PIXELS
    {
        return Err(image_error(
            "入力画像は各辺4096 pixel、合計16777216 pixel以内にしてください。",
        ));
    }
    if decoded_bytes > MAX_DECODED_RASTER_BYTES {
        return Err(image_error(
            "入力画像の展開サイズが処理上限を超えています。",
        ));
    }
    Ok((pixels, decoded_bytes))
}

fn decode_png(bytes: &[u8], info: PngInfo) -> Result<DynamicImage, BackendError> {
    let mut reader = ImageReader::with_format(Cursor::new(bytes), ImageFormat::Png);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_INPUT_DIMENSION);
    limits.max_image_height = Some(MAX_INPUT_DIMENSION);
    limits.max_alloc = Some(MAX_DECODED_RASTER_BYTES as u64);
    reader.limits(limits);
    let image = reader
        .decode()
        .map_err(|_| image_error("入力PNGを復号できません。"))?;
    if image.width() != info.width || image.height() != info.height {
        return Err(image_error("PNGヘッダーと復号後の寸法が一致しません。"));
    }
    Ok(image)
}

fn validate_working_budget(
    input_bytes: usize,
    largest_decoded_image: usize,
    canvas_bytes: usize,
) -> Result<(), BackendError> {
    let working_bytes = input_bytes
        .checked_add(largest_decoded_image)
        .and_then(|bytes| bytes.checked_add(canvas_bytes))
        .and_then(|bytes| bytes.checked_add(MAX_RESIZED_RASTER_BYTES))
        .and_then(|bytes| bytes.checked_add(OUTPUT_PNG_BUFFER_RESERVATION_BYTES))
        .and_then(|bytes| bytes.checked_add(PNG_DECODER_OVERHEAD_BYTES))
        .ok_or_else(|| image_error("画像一覧の作業サイズ計算でoverflowしました。"))?;
    if working_bytes > MAX_WORKING_BYTES {
        return Err(image_error(
            "画像一覧の作業メモリが128 MiBの上限を超えます。",
        ));
    }
    Ok(())
}

fn sheet_layout(image_count: usize) -> Result<SheetLayout, BackendError> {
    if image_count == 0 || image_count > MAX_IMAGE_COUNT {
        return Err(image_error("画像一覧は1枚から16枚まで指定してください。"));
    }
    let columns = image_count.min(COLUMNS);
    let rows = image_count
        .checked_add(COLUMNS - 1)
        .ok_or_else(|| image_error("画像一覧の行数計算でoverflowしました。"))?
        / COLUMNS;
    let width = sheet_dimension(columns)?;
    let height = sheet_dimension(rows)?;
    if width.max(height) > MAX_OUTPUT_LONG_EDGE {
        return Err(image_error("画像一覧の長辺が2048 pixelを超えます。"));
    }
    Ok(SheetLayout {
        columns: u32::try_from(columns)
            .map_err(|_| image_error("画像一覧の列数を変換できません。"))?,
        rows: u32::try_from(rows).map_err(|_| image_error("画像一覧の行数を変換できません。"))?,
        width,
        height,
    })
}

fn sheet_dimension(cells: usize) -> Result<u32, BackendError> {
    let cells =
        u32::try_from(cells).map_err(|_| image_error("画像一覧の寸法を変換できません。"))?;
    OUTER_MARGIN
        .checked_mul(2)
        .and_then(|size| size.checked_add(cells.checked_mul(CELL_WIDTH)?))
        .and_then(|size| size.checked_add(cells.saturating_sub(1).checked_mul(CELL_GAP)?))
        .ok_or_else(|| image_error("画像一覧の寸法計算でoverflowしました。"))
}

fn cell_origin(layout: SheetLayout, index: usize) -> Result<(u32, u32), BackendError> {
    let index =
        u32::try_from(index).map_err(|_| image_error("画像一覧の位置を変換できません。"))?;
    let column = index % layout.columns;
    let row = index / layout.columns;
    let x = OUTER_MARGIN
        .checked_add(
            column
                .checked_mul(CELL_WIDTH + CELL_GAP)
                .ok_or_else(|| image_error("画像一覧の横位置計算でoverflowしました。"))?,
        )
        .ok_or_else(|| image_error("画像一覧の横位置計算でoverflowしました。"))?;
    let y = OUTER_MARGIN
        .checked_add(
            row.checked_mul(CELL_HEIGHT + CELL_GAP)
                .ok_or_else(|| image_error("画像一覧の縦位置計算でoverflowしました。"))?,
        )
        .ok_or_else(|| image_error("画像一覧の縦位置計算でoverflowしました。"))?;
    if column >= layout.columns || row >= layout.rows {
        return Err(image_error("画像一覧の位置がレイアウト範囲外です。"));
    }
    Ok((x, y))
}

fn fit_dimensions(
    width: u32,
    height: u32,
    max_width: u32,
    max_height: u32,
) -> Result<(u32, u32), BackendError> {
    if width == 0 || height == 0 || max_width == 0 || max_height == 0 {
        return Err(image_error("画像の寸法は0より大きくしてください。"));
    }
    if width <= max_width && height <= max_height {
        return Ok((width, height));
    }
    let width_limited = u64::from(width)
        .checked_mul(u64::from(max_height))
        .ok_or_else(|| image_error("縮小後の画像寸法計算でoverflowしました。"))?
        > u64::from(height)
            .checked_mul(u64::from(max_width))
            .ok_or_else(|| image_error("縮小後の画像寸法計算でoverflowしました。"))?;
    if width_limited {
        let height = u64::from(height)
            .checked_mul(u64::from(max_width))
            .ok_or_else(|| image_error("縮小後の画像寸法計算でoverflowしました。"))?
            / u64::from(width);
        Ok((
            max_width,
            u32::try_from(height.max(1))
                .map_err(|_| image_error("縮小後の高さを変換できません。"))?,
        ))
    } else {
        let width = u64::from(width)
            .checked_mul(u64::from(max_height))
            .ok_or_else(|| image_error("縮小後の画像寸法計算でoverflowしました。"))?
            / u64::from(height);
        Ok((
            u32::try_from(width.max(1)).map_err(|_| image_error("縮小後の幅を変換できません。"))?,
            max_height,
        ))
    }
}

fn checked_pixel_count(width: u32, height: u32) -> Result<u64, BackendError> {
    u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or_else(|| image_error("画像の画素数計算でoverflowしました。"))
}

fn checked_rgba_bytes(width: u32, height: u32) -> Result<usize, BackendError> {
    let pixels = checked_pixel_count(width, height)?;
    usize::try_from(pixels)
        .ok()
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| image_error("画像の展開サイズ計算でoverflowしました。"))
}

fn draw_header(image: &mut RgbaImage, cell_x: u32, cell_y: u32, index: usize, timestamp: &str) {
    for y in cell_y..cell_y + HEADER_HEIGHT {
        for x in cell_x..cell_x + CELL_WIDTH {
            image.put_pixel(x, y, HEADER_COLOR);
        }
    }
    let label = format!("{index:02} {timestamp}");
    let mut x = cell_x + CONTENT_INSET + 2;
    let y = cell_y + (HEADER_HEIGHT - GLYPH_HEIGHT) / 2;
    for character in label.bytes() {
        draw_glyph(image, x, y, character);
        x += GLYPH_WIDTH + GLYPH_SPACING;
    }
}

fn draw_glyph(image: &mut RgbaImage, x: u32, y: u32, character: u8) {
    for (row, bits) in glyph_rows(character).into_iter().enumerate() {
        for column in 0..GLYPH_WIDTH {
            let mask = 1u8 << (GLYPH_WIDTH - 1 - column);
            if bits & mask != 0 {
                image.put_pixel(x + column, y + row as u32, TEXT_COLOR);
            }
        }
    }
}

fn glyph_rows(character: u8) -> [u8; 7] {
    match character {
        b'0' => [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
        b'1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        b'2' => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
        b'3' => [
            0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        b'4' => [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        b'5' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b00001, 0b00001, 0b11110,
        ],
        b'6' => [
            0b01110, 0b10000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
        b'7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        b'8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        b'9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110,
        ],
        b':' => [0, 0, 0b00100, 0, 0b00100, 0, 0],
        b'.' => [0, 0, 0, 0, 0, 0b00100, 0b00100],
        b' ' => [0; 7],
        _ => [0; 7],
    }
}

#[derive(Default)]
struct BoundedPngWriter {
    bytes: Vec<u8>,
    exceeded_limit: bool,
}

impl Write for BoundedPngWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let Some(new_len) = self.bytes.len().checked_add(buffer.len()) else {
            self.exceeded_limit = true;
            return Err(io::Error::other("PNG size overflow"));
        };
        if new_len > MAX_OUTPUT_PNG_BYTES {
            self.exceeded_limit = true;
            return Err(io::Error::other("PNG size limit exceeded"));
        }
        self.bytes
            .try_reserve(buffer.len())
            .map_err(|_| io::Error::other("PNG output allocation failed"))?;
        self.bytes.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn image_error(message: &str) -> BackendError {
    BackendError::Request {
        message: message.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use image::ImageBuffer;
    use std::io::Cursor;

    fn png(color: [u8; 4], width: u32, height: u32) -> Vec<u8> {
        let image = ImageBuffer::from_pixel(width, height, Rgba(color));
        let mut cursor = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(image)
            .write_to(&mut cursor, ImageFormat::Png)
            .expect("試験PNGを符号化する");
        cursor.into_inner()
    }

    fn png_with_padding(color: [u8; 4], width: u32, height: u32) -> Vec<u8> {
        let encoded = png(color, width, height);
        let padding_len = MAX_INPUT_PNG_BYTES
            .checked_sub(encoded.len())
            .and_then(|remaining| remaining.checked_sub(12))
            .expect("試験PNGに上限までの余白がある");
        let padding = vec![0; padding_len];
        let chunk_type = b"npAD";
        let checksum = png_chunk_crc32(chunk_type, &padding);
        let iend_start = encoded.len().checked_sub(12).expect("IENDがある");
        assert_eq!(&encoded[iend_start + 4..iend_start + 8], b"IEND");

        let mut padded = Vec::with_capacity(MAX_INPUT_PNG_BYTES);
        padded.extend_from_slice(&encoded[..iend_start]);
        padded.extend_from_slice(
            &u32::try_from(padding_len)
                .expect("PNGチャンク長を変換する")
                .to_be_bytes(),
        );
        padded.extend_from_slice(chunk_type);
        padded.extend_from_slice(&padding);
        padded.extend_from_slice(&checksum.to_be_bytes());
        padded.extend_from_slice(&encoded[iend_start..]);
        padded
    }

    fn png_chunk_crc32(chunk_type: &[u8], data: &[u8]) -> u32 {
        const TABLE: [u32; 16] = [
            0x0000_0000,
            0x1db7_1064,
            0x3b6e_20c8,
            0x26d9_30ac,
            0x76dc_4190,
            0x6b6b_51f4,
            0x4db2_6158,
            0x5005_713c,
            0xedb8_8320,
            0xf00f_9344,
            0xd6d6_a3e8,
            0xcb61_b38c,
            0x9b64_c2b0,
            0x86d3_d2d4,
            0xa00a_e278,
            0xbdbd_f21c,
        ];
        let mut crc = u32::MAX;
        for byte in chunk_type.iter().chain(data) {
            crc ^= u32::from(*byte);
            crc = (crc >> 4) ^ TABLE[(crc & 0x0f) as usize];
            crc = (crc >> 4) ^ TABLE[(crc & 0x0f) as usize];
        }
        !crc
    }

    fn request_error(error: BackendError) -> String {
        match error {
            BackendError::Request { message } => message,
            other => other.to_string(),
        }
    }

    #[test]
    fn image_list_keeps_input_order_and_renders_time_headers_in_the_header_band() {
        let colors = [
            [220, 20, 30, 255],
            [20, 180, 40, 255],
            [30, 50, 220, 255],
            [220, 180, 20, 255],
            [190, 30, 180, 255],
        ];
        let frames = colors
            .iter()
            .enumerate()
            .map(|(index, color)| {
                let timestamp = format!("00:00:{index:02}.000");
                let bytes = png(*color, 8, 4);
                (timestamp, bytes)
            })
            .collect::<Vec<_>>();
        let borrowed = frames
            .iter()
            .map(|(timestamp, png)| TimestampedImage { timestamp, png })
            .collect::<Vec<_>>();

        let list = build_timestamped_image_list(&borrowed).expect("画像一覧を作る");
        assert_eq!((list.width, list.height), (2048, 1032));
        assert!(list.png.len() <= MAX_OUTPUT_PNG_BYTES);
        assert_eq!(
            list.text,
            "画像一覧（入力順）\n01: 00:00:00.000\n02: 00:00:01.000\n03: 00:00:02.000\n04: 00:00:03.000\n05: 00:00:04.000"
        );

        let rendered = ImageReader::new(Cursor::new(&list.png))
            .with_guessed_format()
            .expect("一覧PNGの形式を読む")
            .decode()
            .expect("一覧PNGを復号する")
            .into_rgba8();
        for (index, color) in colors.iter().enumerate() {
            let cell_x = OUTER_MARGIN + (index as u32 % 4) * (CELL_WIDTH + CELL_GAP);
            let cell_y = OUTER_MARGIN + (index as u32 / 4) * (CELL_HEIGHT + CELL_GAP);
            let x = cell_x + CONTENT_INSET + (CONTENT_WIDTH - 8) / 2 + 3;
            let y = cell_y + HEADER_HEIGHT + CONTENT_INSET + (CONTENT_HEIGHT - 4) / 2 + 2;
            assert_eq!(rendered.get_pixel(x, y).0, *color, "frame {index} position");
        }

        let first_header_y = OUTER_MARGIN + (HEADER_HEIGHT - GLYPH_HEIGHT) / 2;
        let first_zero_pixel_x = OUTER_MARGIN + CONTENT_INSET + 2 + 1;
        assert_eq!(
            rendered.get_pixel(first_zero_pixel_x, first_header_y).0,
            TEXT_COLOR.0,
            "1始まりの番号の先頭字形が見出し上端の所定位置にある"
        );
        let first_colon_x =
            OUTER_MARGIN + CONTENT_INSET + 2 + 5 * (GLYPH_WIDTH + GLYPH_SPACING) + 2;
        assert_eq!(
            rendered.get_pixel(first_colon_x, first_header_y + 2).0,
            TEXT_COLOR.0,
            "時刻のコロンが見出し内に描かれる"
        );

        let second_number_x = OUTER_MARGIN + CELL_WIDTH + CELL_GAP + CONTENT_INSET + 2;
        assert_eq!(
            rendered.get_pixel(second_number_x, first_header_y + 2).0,
            TEXT_COLOR.0,
            "2枚目の見出し番号の0を正しい位置に描画する"
        );
        assert_eq!(
            rendered
                .get_pixel(
                    second_number_x + GLYPH_WIDTH + GLYPH_SPACING,
                    first_header_y + 2
                )
                .0,
            HEADER_COLOR.0,
            "2枚目の見出し番号の2を正しい位置に描画する"
        );
    }

    #[test]
    fn maximum_dimension_image_fits_the_full_working_budget() {
        let large = png_with_padding([170, 35, 75, 255], 4096, 4096);
        let small = png_with_padding([25, 180, 60, 255], 1, 1);
        assert_eq!(large.len(), MAX_INPUT_PNG_BYTES);
        assert_eq!(small.len(), MAX_INPUT_PNG_BYTES);

        let mut images = Vec::with_capacity(MAX_IMAGE_COUNT);
        images.push(large);
        for _ in 1..MAX_IMAGE_COUNT {
            images.push(small.clone());
        }
        drop(small);
        assert!(images
            .iter()
            .all(|image| image.len() == MAX_INPUT_PNG_BYTES));

        let timestamps = (0..MAX_IMAGE_COUNT)
            .map(|index| format!("00:00:{index:02}.000"))
            .collect::<Vec<_>>();
        let frames = timestamps
            .iter()
            .zip(&images)
            .map(|(timestamp, png)| TimestampedImage { timestamp, png })
            .collect::<Vec<_>>();

        let list = build_timestamped_image_list(&frames)
            .expect("最大寸法と最大入力サイズの一覧を生成する");
        assert_eq!((list.width, list.height), (2048, 2048));
        assert!(list.png.len() <= MAX_OUTPUT_PNG_BYTES);

        let rendered = ImageReader::new(Cursor::new(&list.png))
            .with_guessed_format()
            .expect("一覧PNGの形式を読む")
            .decode()
            .expect("一覧PNGを復号する")
            .into_rgba8();
        let (width, height) = fit_dimensions(4096, 4096, CONTENT_WIDTH, CONTENT_HEIGHT)
            .expect("最大寸法画像を縮小する");
        let image_x = OUTER_MARGIN + CONTENT_INSET + (CONTENT_WIDTH - width) / 2;
        let image_y = OUTER_MARGIN + HEADER_HEIGHT + CONTENT_INSET + (CONTENT_HEIGHT - height) / 2;
        assert_eq!(
            rendered
                .get_pixel(image_x + width / 2, image_y + height / 2)
                .0,
            [170, 35, 75, 255]
        );
    }

    #[test]
    fn sixteen_images_fit_the_long_edge_limit_and_seventeenth_is_rejected() {
        let image = png([80, 120, 160, 255], 2, 2);
        let timestamps = (0..17)
            .map(|index| format!("00:00:{index:02}.000"))
            .collect::<Vec<_>>();
        let frames = timestamps
            .iter()
            .take(16)
            .map(|timestamp| TimestampedImage {
                timestamp,
                png: &image,
            })
            .collect::<Vec<_>>();
        let list = build_timestamped_image_list(&frames).expect("16枚を受け付ける");
        assert_eq!((list.width, list.height), (2048, 2048));
        assert!(list.width.max(list.height) <= MAX_OUTPUT_LONG_EDGE);

        let too_many = timestamps
            .iter()
            .map(|timestamp| TimestampedImage {
                timestamp,
                png: &image,
            })
            .collect::<Vec<_>>();
        let error = build_timestamped_image_list(&too_many).expect_err("17枚を拒否する");
        assert!(request_error(error).contains("最大16枚"));
    }

    #[test]
    fn empty_input_invalid_time_and_malformed_png_are_rejected() {
        let error = build_timestamped_image_list(&[]).expect_err("空の一覧を拒否する");
        assert!(request_error(error).contains("画像一覧"));

        let image = png([1, 2, 3, 255], 1, 1);
        for timestamp in [
            "0:00:00.000",
            "00:60:00.000",
            "00:00:60.000",
            "00:00:00,000",
        ] {
            let error = build_timestamped_image_list(&[TimestampedImage {
                timestamp,
                png: &image,
            }])
            .expect_err("不正な時刻を拒否する");
            assert!(request_error(error).contains("時刻"));
        }

        let error = build_timestamped_image_list(&[TimestampedImage {
            timestamp: "00:00:00.000",
            png: b"not png",
        }])
        .expect_err("不正PNGを拒否する");
        assert!(request_error(error).contains("PNG"));
    }

    #[test]
    fn image_dimension_and_raster_size_overflow_are_rejected() {
        let mut oversized = png([1, 2, 3, 255], 1, 1);
        oversized[16..20].copy_from_slice(&u32::MAX.to_be_bytes());
        oversized[20..24].copy_from_slice(&u32::MAX.to_be_bytes());
        let error = build_timestamped_image_list(&[TimestampedImage {
            timestamp: "00:00:00.000",
            png: &oversized,
        }])
        .expect_err("寸法overflowを含む入力を拒否する");
        assert!(request_error(error).contains("overflow"));
        assert!(checked_rgba_bytes(u32::MAX, u32::MAX).is_err());
    }

    #[test]
    fn input_dimension_edge_and_pixel_caps_are_enforced_before_decode() {
        let at_limit = PngInfo {
            width: MAX_INPUT_DIMENSION,
            height: MAX_INPUT_DIMENSION,
            bit_depth: 8,
            color_type: 6,
        };
        assert_eq!(
            validate_input_dimensions(at_limit).expect("4096×4096を受け付ける"),
            (MAX_INPUT_PIXELS, MAX_DECODED_RASTER_BYTES)
        );

        let over_limit = PngInfo {
            width: MAX_INPUT_DIMENSION + 1,
            height: 1,
            bit_depth: 8,
            color_type: 6,
        };
        let error =
            validate_input_dimensions(over_limit).expect_err("4096 pixelを超える辺を拒否する");
        assert!(request_error(error).contains("4096"));
    }

    #[test]
    fn input_png_byte_limit_is_checked_before_decoding() {
        let oversized = vec![0; MAX_INPUT_PNG_BYTES + 1];
        let error = build_timestamped_image_list(&[TimestampedImage {
            timestamp: "00:00:00.000",
            png: &oversized,
        }])
        .expect_err("入力PNGの2 MiB上限を超える画像を拒否する");
        assert!(request_error(error).contains("2 MiB"));
    }

    #[test]
    fn output_writer_never_retains_bytes_past_the_png_limit() {
        let mut writer = BoundedPngWriter::default();
        writer
            .write_all(&vec![0; MAX_OUTPUT_PNG_BYTES])
            .expect("上限ちょうどを受け付ける");
        assert_eq!(writer.bytes.len(), MAX_OUTPUT_PNG_BYTES);
        assert!(writer.write_all(&[1]).is_err());
        assert!(writer.exceeded_limit);
        assert_eq!(writer.bytes.len(), MAX_OUTPUT_PNG_BYTES);
    }

    #[test]
    fn layout_and_scaling_reject_zero_and_keep_dimensions_bounded() {
        assert!(sheet_layout(0).is_err());
        assert!(fit_dimensions(0, 1, CONTENT_WIDTH, CONTENT_HEIGHT).is_err());
        assert_eq!(
            fit_dimensions(4096, 2048, CONTENT_WIDTH, CONTENT_HEIGHT).expect("画像を縮小する"),
            (CONTENT_WIDTH, CONTENT_WIDTH / 2)
        );
    }

    #[test]
    fn memory_budget_accepts_the_documented_peak_and_rejects_overflow() {
        assert!(
            validate_working_budget(32 * 1024 * 1024, 64 * 1024 * 1024, 16 * 1024 * 1024).is_ok()
        );
        assert!(validate_working_budget(usize::MAX, 1, 1).is_err());
        assert!(
            validate_working_budget(35 * 1024 * 1024, 64 * 1024 * 1024, 16 * 1024 * 1024).is_err()
        );
    }
}
