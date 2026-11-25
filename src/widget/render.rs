use std::{ffi::c_void, mem::size_of, ptr::null_mut};
use windows_sys::Win32::{Foundation::*, Graphics::{Gdi::*, GdiPlus::*}, UI::{Shell::SHCreateMemStream, WindowsAndMessaging::*}};

const PARGB32: i32 = 0x000E200B;
const ASSET_IDLE: &[u8] = include_bytes!("../../assets/mascot/gpt-dragon/layers-v2/body-idle.png");
const ASSET_HAPPY: &[u8] = include_bytes!("../../assets/mascot/gpt-dragon/layers-v2/body-happy.png");
const ASSET_WING: &[u8] = include_bytes!("../../assets/mascot/gpt-dragon/layers-v2/wing-left.png");

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

pub struct GdiPlus(usize);
impl GdiPlus {
    pub fn start() -> Option<Self> {
        let mut token = 0;
        let input = GdiplusStartupInput { GdiplusVersion: 1, ..Default::default() };
        (unsafe { GdiplusStartup(&mut token, &input, null_mut()) } == 0).then_some(Self(token))
    }
}
impl Drop for GdiPlus {
    fn drop(&mut self) { unsafe { GdiplusShutdown(self.0) } }
}

struct Stream(*mut c_void);
impl Drop for Stream {
    fn drop(&mut self) {
        // IStream inherits IUnknown. Release is the third vtable entry.
        unsafe {
            let table = *self.0.cast::<*const *const c_void>();
            let release: unsafe extern "system" fn(*mut c_void) -> u32 =
                std::mem::transmute(*table.add(2));
            release(self.0);
        }
    }
}

struct Sprite {
    image: *mut GpBitmap,
    _stream: Option<Stream>,
}
impl Sprite {
    fn load(bytes: &[u8]) -> Option<Self> {
        unsafe {
            let ptr = SHCreateMemStream(bytes.as_ptr(), bytes.len().try_into().ok()?);
            if ptr.is_null() { return None; }
            let stream = Stream(ptr);
            let mut image = null_mut();
            (GdipCreateBitmapFromStream(ptr, &mut image) == 0).then(|| Self { image, _stream: Some(stream) })
        }
    }

    fn resized(&self, width: i32, height: i32, dark: bool) -> Option<Self> {
        unsafe {
            let mut bitmap = null_mut();
            if GdipCreateBitmapFromScan0(width, height, 0, PARGB32, null_mut(), &mut bitmap) != 0 {
                return None;
            }
            let copy = Self { image: bitmap, _stream: None };
            let mut graphics = null_mut();
            if GdipGetImageGraphicsContext(bitmap.cast(), &mut graphics) != 0 { return None; }
            GdipSetInterpolationMode(graphics, InterpolationModeHighQualityBicubic);
            let result = GdipDrawImageRectI(graphics, self.image.cast(), 0, 0, width, height);
            GdipDeleteGraphics(graphics);
            if result != 0 { return None; }
            if dark {
                let bounds = Rect { X: 0, Y: 0, Width: width, Height: height };
                let mut pixels = BitmapData::default();
                let flags = (ImageLockModeRead | ImageLockModeWrite) as u32;
                if GdipBitmapLockBits(bitmap, &bounds, flags, PARGB32, &mut pixels) != 0 {
                    return None;
                }
                // Scale the premultiplied RGB channels to 90% while preserving alpha.
                for row in 0..height {
                    let start = pixels.Scan0.cast::<u8>().offset(row as isize * pixels.Stride as isize);
                    for pixel in std::slice::from_raw_parts_mut(start, width as usize * 4).chunks_exact_mut(4) {
                        for channel in &mut pixel[..3] {
                            *channel = ((u16::from(*channel) * 9 + 5) / 10) as u8;
                        }
                    }
                }
                if GdipBitmapUnlockBits(bitmap, &mut pixels) != 0 { return None; }
            }
            Some(copy)
        }
    }

    fn draw(&self, g: *mut GpGraphics, x: f32, y: f32, w: f32, h: f32) {
        unsafe { GdipDrawImageRect(g, self.image.cast(), x, y, w, h); }
    }
}
impl Drop for Sprite {
    fn drop(&mut self) { unsafe { GdipDisposeImage(self.image.cast()); } }
}

struct ArtLayers {
    idle: Sprite,
    happy: Sprite,
    wing: Sprite,
}
pub struct Art {
    light: ArtLayers,
    dark: ArtLayers,
}

impl Art {
    pub fn load() -> Option<Self> {
        let idle = Sprite::load(ASSET_IDLE)?;
        let happy = Sprite::load(ASSET_HAPPY)?;
        let wing = Sprite::load(ASSET_WING)?;
        let layers = |dark| -> Option<ArtLayers> {
            Some(ArtLayers {
                idle: idle.resized(512, 512, dark)?,
                happy: happy.resized(512, 512, dark)?,
                wing: wing.resized(200, 125, dark)?,
            })
        };
        Some(Self { light: layers(false)?, dark: layers(true)? })
    }

    pub fn draw(&self, g: &Graphics<'_>, y: f32, float: f32, angle: f32, pressed: bool, happy: bool, dark: bool) {
        let layers = if dark { &self.dark } else { &self.light };
        let g = g.0;
        unsafe {
            let mut initial = 0;
            GdipSaveGraphics(g, &mut initial);
            GdipTranslateWorldTransform(g, 110.0, y + 105.0 + float, MatrixOrderPrepend);
            if pressed { GdipScaleWorldTransform(g, 1.008, 0.990, MatrixOrderPrepend); }
            let scale = 184.0 / 1254.0;
            let wing_x = -92.0 + 25.0 * scale;
            let wing_y = -92.0 + 865.0 * scale;
            let (w, h) = (475.0 * scale, 297.1 * scale);
            for mirror in [1.0, -1.0] {
                let mut state = 0;
                GdipSaveGraphics(g, &mut state);
                GdipScaleWorldTransform(g, mirror, 1.0, MatrixOrderPrepend);
                GdipTranslateWorldTransform(g, wing_x + w * 0.93, wing_y + h * 0.56, MatrixOrderPrepend);
                GdipRotateWorldTransform(g, angle, MatrixOrderPrepend);
                layers.wing.draw(g, -w * 0.93, -h * 0.56, w, h);
                GdipRestoreGraphics(g, state);
            }
            let body = if happy { &layers.happy } else { &layers.idle };
            body.draw(g, -92.0, -92.0, 184.0, 184.0);
            GdipRestoreGraphics(g, initial);
        }
    }
}

pub struct Surface {
    dc: HDC,
    dib: HBITMAP,
    previous: HGDIOBJ,
    bits: *mut c_void,
    size: SIZE,
    text: TextResources,
}

impl Surface {
    pub fn new(w: i32, h: i32) -> Option<Self> {
        unsafe {
            let dc = CreateCompatibleDC(null_mut());
            if dc.is_null() { return None; }
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER { biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, ..Default::default() },
                ..Default::default()
            };
            let mut bits = null_mut();
            let dib = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
            if dib.is_null() {
                DeleteDC(dc);
                return None;
            }
            let previous = SelectObject(dc, dib);
            if previous.is_null() {
                DeleteObject(dib);
                DeleteDC(dc);
                return None;
            }
            Some(Self { dc, dib, previous, bits, size: SIZE { cx: w, cy: h }, text: TextResources::new() })
        }
    }

    pub fn draw(&mut self, scale: f32, paint: impl FnOnce(&Graphics<'_>)) -> bool {
        unsafe {
            let mut target = null_mut();
            if GdipCreateBitmapFromScan0(self.size.cx, self.size.cy, self.size.cx * 4, PARGB32,
                self.bits.cast(), &mut target) != 0 { return false; }
            let mut g = null_mut();
            let ok = GdipGetImageGraphicsContext(target.cast(), &mut g) == 0;
            if ok {
                GdipGraphicsClear(g, 0);
                GdipSetSmoothingMode(g, SmoothingModeAntiAlias);
                GdipSetInterpolationMode(g, InterpolationModeBilinear);
                GdipSetTextRenderingHint(g, TextRenderingHintAntiAliasGridFit);
                GdipScaleWorldTransform(g, scale, scale, MatrixOrderPrepend);
                paint(&Graphics(g, &self.text));
                GdipDeleteGraphics(g);
            }
            GdipDisposeImage(target.cast());
            ok
        }
    }

    pub fn present(&self, hwnd: HWND, position: POINT) -> bool {
        let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0,
            SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
        unsafe {
            UpdateLayeredWindow(hwnd, null_mut(), &position, &self.size, self.dc,
                &POINT::default(), 0, &blend, ULW_ALPHA) != 0
        }
    }

    pub fn hit(&self, x: i32, y: i32) -> bool {
        if x < 0 || y < 0 || x >= self.size.cx || y >= self.size.cy { return false; }
        unsafe { (*self.bits.cast::<u32>().add((y * self.size.cx + x) as usize) >> 24) > 18 }
    }
}
impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.previous);
            DeleteObject(self.dib);
            DeleteDC(self.dc);
        }
    }
}

pub struct Graphics<'a>(*mut GpGraphics, &'a TextResources);

struct TextResources {
    regular: *mut GpFont,
    title: *mut GpFont,
    bold: *mut GpFont,
    bold_title: *mut GpFont,
    centered: *mut GpStringFormat,
}
impl TextResources {
    fn new() -> Self {
        let mut text = Self { regular: null_mut(), title: null_mut(), bold: null_mut(), bold_title: null_mut(), centered: null_mut() };
        unsafe {
            let mut family = null_mut();
            if GdipCreateFontFamilyFromName(wide("Microsoft YaHei UI").as_ptr(), null_mut(), &mut family) != 0
                && GdipGetGenericFontFamilySansSerif(&mut family) != 0 { return text; }
            GdipCreateFont(family, 11.0, FontStyleRegular, UnitPixel, &mut text.regular);
            GdipCreateFont(family, 15.0, FontStyleRegular, UnitPixel, &mut text.title);
            GdipCreateFont(family, 11.0, FontStyleBold, UnitPixel, &mut text.bold);
            GdipCreateFont(family, 15.0, FontStyleBold, UnitPixel, &mut text.bold_title);
            GdipDeleteFontFamily(family);
            if GdipCreateStringFormat(0, 0, &mut text.centered) == 0 {
                GdipSetStringFormatAlign(text.centered, StringAlignmentCenter);
            }
        }
        text
    }
}

impl TextResources {
    fn font(&self, size: f32, bold: bool) -> *mut GpFont {
        let (regular, heavy) = if size > 11.0 {
            (self.title, self.bold_title)
        } else {
            (self.regular, self.bold)
        };
        if bold && !heavy.is_null() { heavy } else { regular }
    }
}

impl Drop for TextResources {
    fn drop(&mut self) {
        unsafe {
            if !self.centered.is_null() { GdipDeleteStringFormat(self.centered); }
            if !self.regular.is_null() { GdipDeleteFont(self.regular); }
            if !self.title.is_null() { GdipDeleteFont(self.title); }
            if !self.bold.is_null() { GdipDeleteFont(self.bold); }
            if !self.bold_title.is_null() { GdipDeleteFont(self.bold_title); }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mascot_and_bubble_render_into_the_layered_window_buffer() {
        let _runtime = GdiPlus::start().unwrap();
        let art = Art::load().unwrap();
        let mut surface = Surface::new(220, 294).unwrap();
        let alpha_frame = |surface: &mut Surface, angle| {
            assert!(surface.draw(1.0, |g| {
                art.draw(g, 90.0, 0.0, angle, false, false, false);
                g.bubble(1.0, true, (Some(75), "9/24 00:00"), (Some(40), "9/26 00:00"), "演示", false, true);
            }));
            assert!(!surface.hit(0, 0));
            assert!(surface.hit(110, 40));
            assert!(surface.hit(110, 190));
            (0..294)
                .flat_map(|y| (0..220).map(move |x| (x, y)))
                .map(|(x, y)| surface.hit(x, y))
                .collect::<Vec<_>>()
        };
        assert_ne!(alpha_frame(&mut surface, -16.0), alpha_frame(&mut surface, 16.0));
    }

    #[test]
    fn compact_styles_and_mascot_percentages_render_with_quota_colors() {
        let _runtime = GdiPlus::start().unwrap();
        let mut compact = Surface::new(84, 84).unwrap();
        assert!(compact.draw(1.0, |g| g.double_ring(Some(80), Some(60), false)));
        assert!(!compact.hit(0, 0));
        assert!(compact.hit(42, 42));
        assert_eq!(unsafe { *compact.bits.cast::<u32>().add(4 * 84 + 42) }, 0xFFFFFFFF);

        let mut bar = Surface::new(260, 48).unwrap();
        assert!(bar.draw(1.0, |g| g.horizontal(Some(45), Some(20), true)));
        assert!(!bar.hit(0, 0));
        assert!(bar.hit(237, 24));
        assert!(!bar.hit(10, 4));
        assert_eq!(unsafe { *bar.bits.cast::<u32>().add(24 * 260 + 103) }, 0xFF272037);
        assert!(bar.draw(1.0, |g| g.horizontal(Some(45), Some(20), false)));
        assert_eq!(unsafe { *bar.bits.cast::<u32>().add(24 * 260 + 103) }, 0xFFFFFFFF);
        assert!(compact.draw(1.0, |g| g.double_ring(Some(80), Some(60), true)));
        assert_eq!(unsafe { *compact.bits.cast::<u32>().add(4 * 84 + 42) }, 0xFF272037);

        let mut tooltip = Surface::new(220, 90).unwrap();
        for (dark, expected) in [(false, 0xFFFFFFFF), (true, 0xFF272037)] {
            assert!(tooltip.draw(1.0, |g| {
                g.bubble(1.0, true, (None, "—"), (None, "—"), "", dark, false);
            }));
            assert_eq!(unsafe { *tooltip.bits.cast::<u32>().add(72 * 220 + 110) }, expected);
        }

        let mut detail = Surface::new(220, 140).unwrap();
        assert!(detail.draw(1.0, |g| {
            g.bubble(1.0, true, (Some(48), "11/24 19:14"), (Some(59), "11/30 10:16"), "今日用量", false, true);
            g.caption(Some(48), Some(59), false, 106.0, false);
        }));
        let pixels = unsafe { std::slice::from_raw_parts(detail.bits.cast::<u32>(), 220 * 140) };
        for (from, to) in [(11, 34), (107, 130)] {
            let rows = &pixels[from * 220..to * 220];
            for percent in [48, 59] {
                let target = quota_color(Some(percent), false);
                assert!(rows.iter().any(|&pixel| {
                    [0, 8, 16].iter().all(|shift|
                        ((pixel >> shift) & 255).abs_diff((target >> shift) & 255) <= 14)
                }), "missing quota color for {percent}% in rows {from}..{to}");
            }
        }
    }
}

impl Graphics<'_> {
    pub fn offset(&self, x: f32, y: f32, paint: impl FnOnce()) {
        unsafe {
            let mut state = 0;
            GdipSaveGraphics(self.0, &mut state);
            GdipTranslateWorldTransform(self.0, x, y, MatrixOrderPrepend);
            paint();
            GdipRestoreGraphics(self.0, state);
        }
    }

    fn panel(&self, width: f32, height: f32, dark: bool) {
        unsafe {
            let mut path = null_mut();
            if GdipCreatePath(FillModeWinding, &mut path) != 0 { return; }
            let diameter = height - 2.0;
            if width == height {
                GdipAddPathEllipse(path, 1.0, 1.0, diameter, diameter);
            } else {
                GdipAddPathArc(path, width - height + 1.0, 1.0, diameter, diameter, 270.0, 180.0);
                GdipAddPathArc(path, 1.0, 1.0, diameter, diameter, 90.0, 180.0);
            }
            GdipClosePathFigure(path);
            let mut brush = null_mut();
            if GdipCreateSolidFill(panel_background(dark), &mut brush) == 0 {
                GdipFillPath(self.0, brush.cast(), path);
                GdipDeleteBrush(brush.cast());
            }
            let mut pen = null_mut();
            if GdipCreatePen1(if dark { 0xFF4B4B52 } else { 0xFFDAD4E2 }, 1.0, UnitPixel, &mut pen) == 0 {
                GdipDrawPath(self.0, pen, path);
                GdipDeletePen(pen);
            }
            GdipDeletePath(path);
        }
    }

    pub fn double_ring(&self, five: Option<u32>, week: Option<u32>, dark: bool) {
        self.panel(84.0, 84.0, dark);
        self.ring((42.0, 42.0), 34.0, five, dark);
        self.ring((42.0, 42.0), 25.0, week, dark);
        self.number(five, 20.0, 26.0, 44.0, 15.0, dark);
        self.number(week, 20.0, 46.0, 44.0, 11.0, dark);
    }

    pub fn horizontal(&self, five: Option<u32>, week: Option<u32>, dark: bool) {
        self.panel(260.0, 48.0, dark);
        let muted = if dark { 0xFFD6D3DD } else { 0xFF645773 };
        for (x, label, percent) in [(33.0, "5HOUR", five), (140.0, "WEEK", week)] {
            self.ring((x, 24.0), 13.0, percent, dark);
            self.text(label, x + 18.0, 5.0, 57.0, 11.0, muted);
            self.number(percent, x + 18.0, 21.0, 57.0, 15.0, dark);
        }
        unsafe {
            let mut pen = null_mut();
            if GdipCreatePen1(if dark { 0xFF4B4B52 } else { 0xFFDAD4E2 }, 1.0, UnitPixel, &mut pen) == 0 {
                for x in [113.0, 217.0] { GdipDrawLine(self.0, pen, x, 12.0, x, 36.0); }
                GdipDeletePen(pen);
            }
        }
        self.theme_icon(dark, muted);
    }

    fn theme_icon(&self, sun: bool, color: u32) {
        // Center of the theme area, aligned with both quota rings.
        self.offset(237.5, 24.0, || unsafe {
            let mut pen = null_mut();
            if GdipCreatePen1(color, 1.45, UnitPixel, &mut pen) != 0 {
                return;
            }
            GdipSetPenStartCap(pen, LineCapRound);
            GdipSetPenEndCap(pen, LineCapRound);
            if sun {
                GdipDrawEllipse(self.0, pen, -3.2, -3.2, 6.4, 6.4);
                for step in 0..8 {
                    let (sine, cosine) = (step as f32 * std::f32::consts::FRAC_PI_4).sin_cos();
                    GdipDrawLine(self.0, pen, 5.0 * cosine, 5.0 * sine, 7.1 * cosine, 7.1 * sine);
                }
            } else {
                let mut path = null_mut();
                if GdipCreatePath(FillModeWinding, &mut path) == 0 {
                    GdipAddPathBezier(path, 2.7, -6.2, -7.0, -8.0, -9.2, 3.0, -2.7, 6.0);
                    GdipAddPathBezier(path, -2.7, 6.0, 1.0, 7.7, 5.7, 5.7, 6.0, 2.8);
                    GdipAddPathBezier(path, 6.0, 2.8, 0.7, 4.3, -2.1, -2.5, 2.7, -6.2);
                    GdipClosePathFigure(path);
                    GdipSetPenLineJoin(pen, LineJoinRound);
                    GdipDrawPath(self.0, pen, path);
                    GdipDeletePath(path);
                }
            }
            GdipDeletePen(pen);
        });
    }

    pub fn bubble(&self, top: f32, above: bool, left: (Option<u32>, &str), right: (Option<u32>, &str), footer: &str, dark: bool, mascot: bool) {
        let g = self.0;
        unsafe {
            let mut brush = null_mut();
            let background = if mascot {
                if dark { 0xFF272037 } else { 0xFFF9F6FF }
            } else {
                panel_background(dark)
            };
            if GdipCreateSolidFill(background, &mut brush) != 0 { return; }
            let mut path = null_mut();
            if GdipCreatePath(FillModeWinding, &mut path) == 0 {
                GdipAddPathArc(path, 8.0, top + 1.0, 24.0, 24.0, 180.0, 90.0);
                if !above {
                    GdipAddPathLine(path, 100.0, top + 1.0, 110.0, top - 8.0);
                    GdipAddPathLine(path, 110.0, top - 8.0, 120.0, top + 1.0);
                }
                GdipAddPathArc(path, 188.0, top + 1.0, 24.0, 24.0, 270.0, 90.0);
                GdipAddPathArc(path, 188.0, top + 55.0, 24.0, 24.0, 0.0, 90.0);
                if above {
                    GdipAddPathLine(path, 120.0, top + 79.0, 110.0, top + 88.0);
                    GdipAddPathLine(path, 110.0, top + 88.0, 100.0, top + 79.0);
                }
                GdipAddPathArc(path, 8.0, top + 55.0, 24.0, 24.0, 90.0, 90.0);
                GdipClosePathFigure(path);
                GdipFillPath(g, brush.cast(), path);
                let mut pen = null_mut();
                if GdipCreatePen1(if dark { 0xFFB5A5C5 } else { 0xFFAEA1BD }, 1.7, UnitPixel, &mut pen) == 0 {
                    GdipDrawPath(g, pen, path);
                    GdipDeletePen(pen);
                }
                GdipDeletePath(path);
            }
            GdipDeleteBrush(brush.cast());
        }
        let primary = if dark { 0xFFF5ECFF } else { 0xFF35264E };
        let muted = if dark { 0xFFD8CAE8 } else { 0xFF645773 };
        let purple = if dark { 0xFFD7B2FF } else { 0xFF7953A9 };
        self.quota_pair("5HOUR", left.0, 63.0, top + 9.0, 15.0, primary, dark);
        self.quota_pair("WEEK", right.0, 157.0, top + 9.0, 15.0, primary, dark);
        self.text(left.1, 18.0, top + 29.0, 90.0, 11.0, muted);
        self.text(right.1, 112.0, top + 29.0, 90.0, 11.0, muted);
        self.text(footer, 10.0, top + 55.0, 200.0, 11.0, purple);
    }

    fn quota_pair(&self, label: &str, percent: Option<u32>, center: f32, top: f32, size: f32, color: u32, dark: bool) {
        if self.1.centered.is_null() { return; }
        let number = percent_text(percent);
        let width = |text: &str, bold| {
            let font = self.1.font(if bold { size } else { 11.0 }, bold);
            if font.is_null() { return 0.0; }
            let text = wide(text);
            let rect = RectF { X: 0.0, Y: 0.0, Width: 200.0, Height: size + 12.0 };
            let mut bounds = RectF::default();
            unsafe { GdipMeasureString(self.0, text.as_ptr(), -1, font, &rect, self.1.centered,
                &mut bounds, null_mut(), null_mut()); }
            bounds.Width
        };
        let label_width = width(label, false);
        let number_width = width(&number, true);
        let gap = 3.0;
        let left = center - (label_width + gap + number_width) / 2.0;
        self.text(label, left - 1.0, top + (size - 11.0) * 0.75, label_width + 2.0, 11.0, color);
        self.number(percent, left + label_width + gap - 1.0, top, number_width + 2.0, size, dark);
    }

    pub fn caption(&self, five: Option<u32>, week: Option<u32>, stale: bool, top: f32, dark: bool) {
        unsafe {
            let mut brush = null_mut();
            let background = if dark { 0xED272037 } else { 0xEFF9F6FF };
            if GdipCreateSolidFill(background, &mut brush) == 0 {
                let mut path = null_mut();
                if GdipCreatePath(FillModeWinding, &mut path) == 0 {
                    let diameter = 12.0;
                    for (x, y, angle) in [
                        (34.0, top, 180.0),
                        (174.0, top, 270.0),
                        (174.0, top + 11.0, 0.0),
                        (34.0, top + 11.0, 90.0),
                    ] {
                        GdipAddPathArc(path, x, y, diameter, diameter, angle, 90.0);
                    }
                    GdipClosePathFigure(path);
                    GdipFillPath(self.0, brush.cast(), path);
                    GdipDeletePath(path);
                }
                GdipDeleteBrush(brush.cast());
            }
        }
        let foreground = if dark { 0xFFF3E7FF } else { 0xFF67458F };
        self.quota_pair("5HOUR", five, 73.0, top + 3.0, 11.0, foreground, dark);
        self.text("·", 103.0, top + 3.0, 14.0, 11.0, foreground);
        self.quota_pair("WEEK", week, 146.0, top + 3.0, 11.0, foreground, dark);
        if stale { self.text("*", 173.0, top + 3.0, 12.0, 11.0, foreground); }
    }

    pub fn ring(&self, center: (f32, f32), radius: f32, percent: Option<u32>, dark: bool) {
        let (x, y) = center;
        let color = quota_color(percent, dark);
        unsafe {
            let mut pen = null_mut();
            let track = if dark { 0xFF423953 } else { 0xFFE4DCEF };
            if GdipCreatePen1(track, 5.0, UnitPixel, &mut pen) == 0 {
                GdipDrawEllipse(self.0, pen, x - radius, y - radius, 2.0 * radius, 2.0 * radius);
                if let Some(percent) = percent.filter(|p| *p > 0) {
                    GdipSetPenColor(pen, color);
                    GdipSetPenStartCap(pen, LineCapRound);
                    GdipSetPenEndCap(pen, LineCapRound);
                    GdipDrawArc(self.0, pen, x - radius, y - radius, 2.0 * radius, 2.0 * radius, -90.0, 3.6 * percent as f32);
                }
                GdipDeletePen(pen);
            }
        }
    }

    fn text(&self, text: &str, x: f32, y: f32, w: f32, size: f32, color: u32) {
        self.draw_text(text, RectF { X: x, Y: y, Width: w, Height: size + 8.0 }, self.1.font(size, false), color);
    }

    fn number(&self, percent: Option<u32>, x: f32, y: f32, w: f32, size: f32, dark: bool) {
        self.draw_text(&percent_text(percent), RectF { X: x, Y: y, Width: w, Height: size + 8.0 },
            self.1.font(size, true), quota_color(percent, dark));
    }

    fn draw_text(&self, text: &str, rect: RectF, font: *mut GpFont, color: u32) {
        if font.is_null() || self.1.centered.is_null() { return; }
        unsafe {
            let mut brush = null_mut();
            if GdipCreateSolidFill(color, &mut brush) != 0 { return; }
            let text = wide(text);
            GdipDrawString(self.0, text.as_ptr(), -1, font, &rect, self.1.centered, brush.cast());
            GdipDeleteBrush(brush.cast());
        }
    }
}

fn panel_background(dark: bool) -> u32 {
    if dark { 0xFF272037 } else { 0xFFFFFFFF }
}

fn percent_text(percent: Option<u32>) -> String {
    percent.map_or_else(|| "—".into(), |value| format!("{value}%"))
}

fn quota_color(percent: Option<u32>, dark: bool) -> u32 {
    match (percent, dark) {
        (Some(76..), false) => 0xFF5062D1,
        (Some(51..=75), false) => 0xFF258362,
        (Some(26..=50), false) => 0xFFA46A18,
        (Some(_), false) => 0xFFB74669,
        (None, false) => 0xFF756A84,
        (Some(76..), true) => 0xFF94A0FF,
        (Some(51..=75), true) => 0xFF68C79F,
        (Some(26..=50), true) => 0xFFE9B65F,
        (Some(_), true) => 0xFFF082A3,
        (None, true) => 0xFFB9AEC8,
    }
}
