use std::ops::{Range, RangeInclusive};

use domain::LineOrigin;
use gpui::{
    App, Bounds, Element, ElementId, FlexDirection, GlobalElementId, Half as _, HighlightStyle,
    Hitbox, HitboxBehavior, Hsla, InspectorElementId, IntoElement, LayoutId, Length, Pixels, Point,
    ScrollHandle, ShapedLine, SharedString, Style, StyledText, TextAlign, TextLayout, TextStyle,
    Window, fill, point, px, relative, size,
};
use gpui_base::{TextSelectionHandle, TextSelectionRegistration, TextSelectionRun};
use gpui_component::{ThemeColor, ThemeMode};

use super::model::Row;
use super::palette::line_colors;

pub(super) const ROW_HEIGHT: f32 = 18.;

const GUTTER_WIDTH: f32 = 44.;
const MARKER_WIDTH: f32 = 16.;
const GUTTER_PADDING: f32 = 8.;
const MARKER_PADDING: f32 = 5.;
const CODE_LEFT: f32 = 2. * GUTTER_WIDTH + MARKER_WIDTH;
const TRAILING_SPACE: f32 = 16.;

pub(super) struct DiffBody {
    rows: Vec<Row>,
    strings: Vec<SharedString>,
    selection: TextSelectionHandle,
    scroll: ScrollHandle,
    theme: ThemeColor,
    mode: ThemeMode,
    visible: Range<usize>,
    texts: Vec<StyledText>,
    row_bounds: Vec<Bounds<Pixels>>,
}

pub(super) fn body(
    rows: Vec<Row>,
    selection: TextSelectionHandle,
    scroll: ScrollHandle,
    theme: ThemeColor,
    mode: ThemeMode,
) -> DiffBody {
    let strings: Vec<SharedString> = rows.iter().map(row_text).collect();
    DiffBody {
        rows,
        strings,
        selection,
        scroll,
        theme,
        mode,
        visible: 0..0,
        texts: Vec::new(),
        row_bounds: Vec::new(),
    }
}

fn row_text(row: &Row) -> SharedString {
    match row {
        Row::FileHeader { path, stat } => format!("{path}  {stat}").into(),
        Row::HunkHeader { text } => text.clone().into(),
        Row::Line { content, .. } => content.clone().into(),
        Row::Placeholder { message } => (*message).into(),
    }
}

fn styled_row(row: &Row, text: SharedString, theme: &ThemeColor, mode: ThemeMode) -> StyledText {
    let range = 0..text.len();
    let highlight = HighlightStyle {
        color: Some(row_foreground(row, theme, mode)),
        ..Default::default()
    };
    StyledText::new(text).with_highlights([(range, highlight)])
}

fn row_foreground(row: &Row, theme: &ThemeColor, mode: ThemeMode) -> Hsla {
    match row {
        Row::FileHeader { .. } => theme.foreground,
        Row::HunkHeader { .. } | Row::Placeholder { .. } => theme.muted_foreground,
        Row::Line { origin, .. } => line_colors(*origin, mode, theme).foreground,
    }
}

fn row_background(row: &Row, theme: &ThemeColor, mode: ThemeMode) -> Option<Hsla> {
    match row {
        Row::FileHeader { .. } => Some(theme.secondary),
        Row::HunkHeader { .. } => Some(theme.muted),
        Row::Placeholder { .. } => None,
        Row::Line { origin, .. } => line_colors(*origin, mode, theme).background,
    }
}

fn marker(origin: LineOrigin) -> &'static str {
    match origin {
        LineOrigin::Addition => "+",
        LineOrigin::Deletion => "\u{2212}",
        LineOrigin::Context => " ",
    }
}

fn selection_quad_bounds(
    start: Point<Pixels>,
    end: Point<Pixels>,
    bounds: Bounds<Pixels>,
    line_height: Pixels,
) -> Vec<Bounds<Pixels>> {
    if start.y == end.y {
        return vec![Bounds::from_corners(
            start,
            Point::new(end.x, end.y + line_height),
        )];
    }

    let mut quads = vec![Bounds::from_corners(
        start,
        Point::new(bounds.right(), start.y + line_height),
    )];
    if end.y > start.y + line_height {
        quads.push(Bounds::from_corners(
            Point::new(bounds.left(), start.y + line_height),
            Point::new(bounds.right(), end.y),
        ));
    }
    quads.push(Bounds::from_corners(
        Point::new(bounds.left(), end.y),
        Point::new(end.x, end.y + line_height),
    ));
    quads
}

fn row_window(offset_y: Pixels, viewport: Pixels, rows: usize) -> Range<usize> {
    if viewport <= px(0.) {
        return 0..rows;
    }
    let first = ((-offset_y) / px(ROW_HEIGHT)).floor().max(0.) as usize;
    let count = (viewport / px(ROW_HEIGHT)).ceil() as usize + 2;
    first.min(rows)..first.saturating_add(count).min(rows)
}

fn selected_rows(
    origin_y: Pixels,
    top: Pixels,
    bottom: Pixels,
    rows: usize,
) -> Option<RangeInclusive<usize>> {
    let last_row = rows.checked_sub(1)?;
    let row_of = |y: Pixels| ((y - origin_y) / px(ROW_HEIGHT)).floor();
    let first = row_of(top);
    let last = row_of(bottom);
    if first > last_row as f32 || last < 0. {
        return None;
    }
    Some((first.max(0.) as usize)..=(last as usize).min(last_row))
}

fn selection_band(
    row_top: Pixels,
    anchor: Point<Pixels>,
    cursor: Point<Pixels>,
) -> Option<(Pixels, Pixels)> {
    let height = px(ROW_HEIGHT);
    let in_row = |point: Point<Pixels>| point.y >= row_top && point.y < row_top + height;
    if row_top + height <= anchor.y.min(cursor.y) || row_top > anchor.y.max(cursor.y) {
        return None;
    }
    if in_row(anchor) && in_row(cursor) {
        return Some((anchor.x.min(cursor.x), anchor.x.max(cursor.x)));
    }
    let (start, end) = if anchor.y < cursor.y {
        (anchor, cursor)
    } else {
        (cursor, anchor)
    };
    if in_row(start) {
        Some((start.x, px(f32::MAX)))
    } else if in_row(end) {
        Some((px(f32::MIN), end.x))
    } else {
        Some((px(f32::MIN), px(f32::MAX)))
    }
}

fn selected_range(
    text: &str,
    line: &ShapedLine,
    left: Pixels,
    band: (Pixels, Pixels),
) -> Option<Range<usize>> {
    if text.len() != line.len() {
        return None;
    }
    let (low, high) = band;
    let mut range: Option<Range<usize>> = None;
    let mut start = line.x_for_index(0);
    for (offset, character) in text.char_indices() {
        let next = offset + character.len_utf8();
        let end = line.x_for_index(next);
        let middle = left + start + (end - start).half();
        if middle >= low && middle <= high {
            range.get_or_insert(offset..offset).end = next;
        }
        start = end;
    }
    range
}

fn copy_text(strings: &[SharedString], ranges: &[Option<Range<usize>>]) -> String {
    let (Some(first), Some(last)) = (
        ranges.iter().position(Option::is_some),
        ranges.iter().rposition(Option::is_some),
    ) else {
        return String::new();
    };

    strings[first..=last]
        .iter()
        .zip(&ranges[first..=last])
        .map(|(text, range)| match range {
            Some(range) => &text[range.clone()],
            None => "",
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn paint_selection(layout: &TextLayout, range: Range<usize>, color: Hsla, window: &mut Window) {
    let (Some(start), Some(end)) = (
        layout.position_for_index(range.start),
        layout.position_for_index(range.end),
    ) else {
        return;
    };
    for bounds in selection_quad_bounds(start, end, layout.bounds(), layout.line_height()) {
        window.paint_quad(fill(bounds, color));
    }
}

struct Pen {
    style: TextStyle,
    font_size: Pixels,
}

impl Pen {
    fn new(window: &Window) -> Self {
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        Self { style, font_size }
    }

    fn shape(&self, text: SharedString, color: Hsla, window: &Window) -> ShapedLine {
        let mut run = self.style.to_run(text.len());
        run.color = color;
        window
            .text_system()
            .shape_line(text, self.font_size, &[run], None)
    }

    fn measure(&self, text: SharedString, window: &Window) -> ShapedLine {
        self.shape(text, self.style.color, window)
    }

    fn width(&self, text: SharedString, window: &Window) -> Pixels {
        self.measure(text, window).width()
    }
}

fn paint_line(line: &ShapedLine, origin: Point<Pixels>, window: &mut Window, cx: &mut App) {
    let _ = line.paint(origin, px(ROW_HEIGHT), TextAlign::Left, None, window, cx);
}

fn paint_number(
    number: Option<u32>,
    right: Pixels,
    top: Pixels,
    color: Hsla,
    pen: &Pen,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(number) = number else {
        return;
    };
    let line = pen.shape(number.to_string().into(), color, window);
    let origin = point(right - px(GUTTER_PADDING) - line.width(), top);
    paint_line(&line, origin, window, cx);
}

impl DiffBody {
    fn content_width(&self, window: &Window) -> Pixels {
        let pen = Pen::new(window);
        let mut widest = px(0.);
        for text in &self.strings {
            widest = widest.max(pen.width(text.clone(), window));
        }
        px(CODE_LEFT) + widest + px(TRAILING_SPACE)
    }

    fn bounds_for_row(&self, bounds: Bounds<Pixels>, index: usize) -> Bounds<Pixels> {
        Bounds::new(
            point(
                bounds.origin.x + px(CODE_LEFT),
                bounds.origin.y + px(index as f32 * ROW_HEIGHT),
            ),
            size(
                (bounds.size.width - px(CODE_LEFT)).max(px(0.)),
                px(ROW_HEIGHT),
            ),
        )
    }

    fn visible_rows(&self) -> Range<usize> {
        row_window(
            self.scroll.offset().y,
            self.scroll.bounds().size.height,
            self.rows.len(),
        )
    }

    fn copy_selection(
        &self,
        bounds: Bounds<Pixels>,
        projected: &[Option<Range<usize>>],
        pen: &Pen,
        window: &Window,
        cx: &App,
    ) -> String {
        let Some(points) = self
            .selection
            .snapshot(cx)
            .and_then(|snapshot| snapshot.window_points())
        else {
            return String::new();
        };
        let anchor = points.anchor();
        let cursor = points.cursor();
        let Some(rows) = selected_rows(
            bounds.origin.y,
            anchor.y.min(cursor.y),
            anchor.y.max(cursor.y),
            self.rows.len(),
        ) else {
            return String::new();
        };

        let left = bounds.origin.x + px(CODE_LEFT);
        let ranges: Vec<Option<Range<usize>>> = rows
            .clone()
            .map(|index| {
                if self.visible.contains(&index) {
                    return projected
                        .get(index - self.visible.start)
                        .and_then(Clone::clone);
                }
                let row_top = bounds.origin.y + px(index as f32 * ROW_HEIGHT);
                let band = selection_band(row_top, anchor, cursor)?;
                let text = &self.strings[index];
                selected_range(text, &pen.measure(text.clone(), window), left, band)
            })
            .collect();

        copy_text(&self.strings[rows], &ranges)
    }

    fn paint_gutter(
        &self,
        index: usize,
        left: Pixels,
        top: Pixels,
        pen: &Pen,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Row::Line {
            origin,
            old_number,
            new_number,
            ..
        } = self.rows[index]
        else {
            return;
        };

        let muted = self.theme.muted_foreground;
        paint_number(
            old_number,
            left + px(GUTTER_WIDTH),
            top,
            muted,
            pen,
            window,
            cx,
        );
        paint_number(
            new_number,
            left + px(2. * GUTTER_WIDTH),
            top,
            muted,
            pen,
            window,
            cx,
        );

        let foreground = line_colors(origin, self.mode, &self.theme).foreground;
        let line = pen.shape(marker(origin).into(), foreground, window);
        paint_line(
            &line,
            point(left + px(2. * GUTTER_WIDTH + MARKER_PADDING), top),
            window,
            cx,
        );
    }
}

impl IntoElement for DiffBody {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for DiffBody {
    type RequestLayoutState = ();
    type PrepaintState = Hitbox;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.visible = self.visible_rows();
        self.texts = self
            .visible
            .clone()
            .map(|index| {
                styled_row(
                    &self.rows[index],
                    self.strings[index].clone(),
                    &self.theme,
                    self.mode,
                )
            })
            .collect();

        let children: Vec<LayoutId> = self
            .texts
            .iter_mut()
            .map(|text| text.request_layout(None, None, window, cx).0)
            .collect();

        let width = self.content_width(window);
        let style = Style {
            flex_direction: FlexDirection::Column,
            flex_shrink: 0.,
            size: size(width.into(), px(self.rows.len() as f32 * ROW_HEIGHT).into()),
            min_size: size(relative(1.).into(), Length::Auto),
            ..Default::default()
        };

        (window.request_layout(style, children, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.row_bounds = self
            .visible
            .clone()
            .map(|index| self.bounds_for_row(bounds, index))
            .collect();
        for (text, row_bounds) in self.texts.iter_mut().zip(&self.row_bounds) {
            text.prepaint(None, None, *row_bounds, &mut (), window, cx);
        }

        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        self.selection.register(
            TextSelectionRegistration::new(hitbox.clone(), bounds)
                .with_document_order(0)
                .with_text_bounds(self.row_bounds.clone()),
            window,
            cx,
        );
        hitbox
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let runs: Vec<TextSelectionRun> = self
            .texts
            .iter()
            .enumerate()
            .map(|(offset, text)| {
                let index = self.visible.start + offset;
                TextSelectionRun::new(
                    self.strings[index].clone(),
                    text.layout().clone(),
                    self.row_bounds[offset],
                )
                .with_document_order(index as u64)
            })
            .collect();
        let projection = self.selection.update_runs(&runs, cx);

        let pen = Pen::new(window);
        let selected = self.copy_selection(bounds, projection.ranges(), &pen, window, cx);
        self.selection.set_fallback_copy_text(selected, cx);

        for (offset, index) in self.visible.clone().enumerate() {
            let row_bounds = self.row_bounds[offset];
            let top = row_bounds.origin.y;

            if let Some(background) = row_background(&self.rows[index], &self.theme, self.mode) {
                let band = Bounds::new(
                    point(bounds.origin.x, top),
                    size(bounds.size.width, px(ROW_HEIGHT)),
                );
                window.paint_quad(fill(band, background));
            }

            if let Some(range) = projection.ranges().get(offset).and_then(Clone::clone) {
                paint_selection(
                    self.texts[offset].layout(),
                    range,
                    self.theme.selection,
                    window,
                );
            }

            self.paint_gutter(index, bounds.origin.x, top, &pen, window, cx);
            self.texts[offset].paint(None, None, row_bounds, &mut (), &mut (), window, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_text_joins_the_selected_rows_with_newlines() {
        let strings = vec![
            SharedString::from("one"),
            SharedString::from("two"),
            SharedString::from("three"),
        ];
        let ranges = vec![Some(1..3), Some(0..3), None];
        assert_eq!(copy_text(&strings, &ranges), "ne\ntwo");
    }

    #[test]
    fn copy_text_keeps_a_blank_row_inside_the_selection() {
        let strings = vec![
            SharedString::from("one"),
            SharedString::from(""),
            SharedString::from("three"),
        ];
        let ranges = vec![Some(0..3), None, Some(0..5)];
        assert_eq!(copy_text(&strings, &ranges), "one\n\nthree");
    }

    #[test]
    fn copy_text_of_an_empty_projection_is_empty() {
        let strings = vec![SharedString::from("one")];
        assert_eq!(copy_text(&strings, &[None]), "");
    }

    #[test]
    fn a_wrapped_selection_covers_the_full_width_of_the_middle_lines() {
        let bounds = Bounds::new(point(px(10.), px(20.)), size(px(100.), px(100.)));
        let quads = selection_quad_bounds(
            point(px(40.), px(20.)),
            point(px(30.), px(80.)),
            bounds,
            px(20.),
        );

        assert_eq!(
            quads,
            vec![
                Bounds::from_corners(point(px(40.), px(20.)), point(px(110.), px(40.))),
                Bounds::from_corners(point(px(10.), px(40.)), point(px(110.), px(80.))),
                Bounds::from_corners(point(px(10.), px(80.)), point(px(30.), px(100.))),
            ]
        );
    }

    #[test]
    fn the_window_is_every_row_until_the_viewport_has_been_measured() {
        assert_eq!(row_window(px(0.), px(0.), 500), 0..500);
    }

    #[test]
    fn the_window_starts_at_the_first_row_the_viewport_cuts_through() {
        let window = row_window(px(-3. * ROW_HEIGHT - 4.), px(10. * ROW_HEIGHT), 500);
        assert_eq!(window.start, 3);
        assert_eq!(window.end, 3 + 12);
    }

    #[test]
    fn the_window_never_runs_past_the_last_row() {
        assert_eq!(
            row_window(px(-490. * ROW_HEIGHT), px(10. * ROW_HEIGHT), 500).end,
            500
        );
        assert_eq!(
            row_window(px(-900. * ROW_HEIGHT), px(10. * ROW_HEIGHT), 500),
            500..500
        );
    }

    #[test]
    fn a_selection_spans_the_rows_its_two_endpoints_land_in() {
        assert_eq!(
            selected_rows(
                px(100.),
                px(100. + 2.5 * ROW_HEIGHT),
                px(100. + 7.1 * ROW_HEIGHT),
                20
            ),
            Some(2..=7)
        );
    }

    #[test]
    fn a_selection_reaching_past_the_body_is_clamped_to_it() {
        assert_eq!(
            selected_rows(px(100.), px(-500.), px(100. + 900. * ROW_HEIGHT), 20),
            Some(0..=19)
        );
    }

    #[test]
    fn a_selection_entirely_outside_the_body_spans_no_rows() {
        assert_eq!(selected_rows(px(100.), px(-500.), px(-400.), 20), None);
        assert_eq!(
            selected_rows(
                px(100.),
                px(100. + 40. * ROW_HEIGHT),
                px(100. + 50. * ROW_HEIGHT),
                20
            ),
            None
        );
        assert_eq!(selected_rows(px(100.), px(100.), px(200.), 0), None);
    }

    #[test]
    fn a_row_holding_both_endpoints_is_bounded_by_them() {
        let band = selection_band(px(36.), point(px(80.), px(40.)), point(px(20.), px(50.)));
        assert_eq!(band, Some((px(20.), px(80.))));
    }

    #[test]
    fn the_first_row_of_a_selection_runs_from_its_endpoint_to_the_end_of_the_line() {
        let band = selection_band(px(36.), point(px(80.), px(40.)), point(px(20.), px(200.)));
        assert_eq!(band, Some((px(80.), px(f32::MAX))));
    }

    #[test]
    fn the_last_row_of_a_selection_runs_from_the_start_of_the_line_to_its_endpoint() {
        let band = selection_band(px(36.), point(px(80.), px(-10.)), point(px(20.), px(40.)));
        assert_eq!(band, Some((px(f32::MIN), px(20.))));
    }

    #[test]
    fn a_row_between_the_endpoints_is_selected_whole() {
        let band = selection_band(px(36.), point(px(80.), px(-10.)), point(px(20.), px(200.)));
        assert_eq!(band, Some((px(f32::MIN), px(f32::MAX))));
    }

    #[test]
    fn a_row_outside_the_selection_has_no_band() {
        let above = selection_band(px(0.), point(px(80.), px(40.)), point(px(20.), px(50.)));
        let below = selection_band(px(90.), point(px(80.), px(40.)), point(px(20.), px(50.)));
        assert_eq!(above, None);
        assert_eq!(below, None);
    }
}
