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
use super::pairing::SideLine;
use super::palette::line_colors;
use super::split::SplitRow;

pub(super) const ROW_HEIGHT: f32 = 18.;

const GUTTER_WIDTH: f32 = 44.;
const MARKER_WIDTH: f32 = 16.;
const GUTTER_PADDING: f32 = 8.;
const MARKER_PADDING: f32 = 5.;
const CODE_LEFT: f32 = 2. * GUTTER_WIDTH + MARKER_WIDTH;
const SPLIT_CODE_LEFT: f32 = GUTTER_WIDTH + MARKER_WIDTH;
const COLUMN_RULE_WIDTH: f32 = 1.;
const TRAILING_SPACE: f32 = 16.;

pub(super) enum Rows {
    Unified(Vec<Row>),
    Split(Vec<SplitRow>),
}

impl Rows {
    fn len(&self) -> usize {
        match self {
            Rows::Unified(rows) => rows.len(),
            Rows::Split(rows) => rows.len(),
        }
    }

    fn columns(&self) -> usize {
        match self {
            Rows::Unified(_) => 1,
            Rows::Split(_) => 2,
        }
    }

    fn code_left(&self) -> f32 {
        match self {
            Rows::Unified(_) => CODE_LEFT,
            Rows::Split(_) => SPLIT_CODE_LEFT,
        }
    }

    fn cells(&self) -> usize {
        self.len() * self.columns()
    }

    fn side(&self, row: usize, column: usize) -> Option<&SideLine> {
        let Rows::Split(rows) = self else {
            return None;
        };
        let SplitRow::Sides { left, right } = &rows[row] else {
            return None;
        };
        if column == 0 {
            left.as_ref()
        } else {
            right.as_ref()
        }
    }
}

pub(super) struct DiffBody {
    rows: Rows,
    strings: Vec<SharedString>,
    selection: TextSelectionHandle,
    scroll: ScrollHandle,
    theme: ThemeColor,
    mode: ThemeMode,
    visible: Range<usize>,
    texts: Vec<StyledText>,
    cell_bounds: Vec<Bounds<Pixels>>,
}

pub(super) fn body(
    rows: Rows,
    selection: TextSelectionHandle,
    scroll: ScrollHandle,
    theme: ThemeColor,
    mode: ThemeMode,
) -> DiffBody {
    let strings: Vec<SharedString> = (0..rows.cells())
        .map(|cell| cell_text(&rows, cell))
        .collect();
    DiffBody {
        rows,
        strings,
        selection,
        scroll,
        theme,
        mode,
        visible: 0..0,
        texts: Vec::new(),
        cell_bounds: Vec::new(),
    }
}

fn cell_text(rows: &Rows, cell: usize) -> SharedString {
    let columns = rows.columns();
    let (row, column) = (cell / columns, cell % columns);
    match rows {
        Rows::Unified(rows) => row_text(&rows[row]),
        Rows::Split(split) => match &split[row] {
            SplitRow::Full(full) if column == 0 => row_text(full),
            SplitRow::Full(_) => SharedString::default(),
            SplitRow::Sides { left, right } => {
                let side = if column == 0 { left } else { right };
                side.as_ref()
                    .map(|side| SharedString::from(side.content.clone()))
                    .unwrap_or_default()
            }
        },
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

fn styled_cell(
    rows: &Rows,
    cell: usize,
    text: SharedString,
    theme: &ThemeColor,
    mode: ThemeMode,
) -> StyledText {
    let range = 0..text.len();
    let highlight = HighlightStyle {
        color: Some(cell_foreground(rows, cell, theme, mode)),
        ..Default::default()
    };
    StyledText::new(text).with_highlights([(range, highlight)])
}

fn cell_foreground(rows: &Rows, cell: usize, theme: &ThemeColor, mode: ThemeMode) -> Hsla {
    let columns = rows.columns();
    let (row, column) = (cell / columns, cell % columns);
    match rows {
        Rows::Unified(rows) => row_foreground(&rows[row], theme, mode),
        Rows::Split(split) => match &split[row] {
            SplitRow::Full(full) => row_foreground(full, theme, mode),
            SplitRow::Sides { .. } => rows
                .side(row, column)
                .map(|side| line_colors(side.origin, mode, theme).foreground)
                .unwrap_or(theme.foreground),
        },
    }
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

#[derive(Clone, Copy, Debug, PartialEq)]
enum Band {
    Whole,
    From(Pixels),
    To(Pixels),
    Between(Pixels, Pixels),
}

impl Band {
    fn holds(self, x: Pixels) -> bool {
        match self {
            Band::Whole => true,
            Band::From(low) => x >= low,
            Band::To(high) => x <= high,
            Band::Between(low, high) => x >= low && x <= high,
        }
    }
}

fn selection_band(row_top: Pixels, anchor: Point<Pixels>, cursor: Point<Pixels>) -> Option<Band> {
    let height = px(ROW_HEIGHT);
    let in_row = |point: Point<Pixels>| point.y >= row_top && point.y < row_top + height;
    if row_top + height <= anchor.y.min(cursor.y) || row_top > anchor.y.max(cursor.y) {
        return None;
    }
    if in_row(anchor) && in_row(cursor) {
        return Some(Band::Between(
            anchor.x.min(cursor.x),
            anchor.x.max(cursor.x),
        ));
    }
    let (start, end) = if anchor.y < cursor.y {
        (anchor, cursor)
    } else {
        (cursor, anchor)
    };
    if in_row(start) {
        Some(Band::From(start.x))
    } else if in_row(end) {
        Some(Band::To(end.x))
    } else {
        Some(Band::Whole)
    }
}

fn selected_range(
    text: &str,
    band: Band,
    left: Pixels,
    line: impl FnOnce() -> ShapedLine,
) -> Option<Range<usize>> {
    if band == Band::Whole {
        return (!text.is_empty()).then_some(0..text.len());
    }
    let line = line();
    if text.len() != line.len() {
        return None;
    }
    let mut range: Option<Range<usize>> = None;
    let mut start = line.x_for_index(0);
    for (offset, character) in text.char_indices() {
        let next = offset + character.len_utf8();
        let end = line.x_for_index(next);
        if band.holds(left + start + (end - start).half()) {
            range.get_or_insert(offset..offset).end = next;
        }
        start = end;
    }
    range
}

fn copy_text(
    strings: &[SharedString],
    ranges: &[Option<Range<usize>>],
    start: usize,
    columns: usize,
) -> String {
    let (Some(first), Some(last)) = (
        ranges.iter().position(Option::is_some),
        ranges.iter().rposition(Option::is_some),
    ) else {
        return String::new();
    };

    let mut text = String::new();
    for cell in first..=last {
        if cell > first {
            text.push(if (start + cell).is_multiple_of(columns) {
                '\n'
            } else {
                '\t'
            });
        }
        if let Some(range) = &ranges[cell] {
            text.push_str(&strings[cell][range.clone()]);
        }
    }
    text
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
        (px(self.rows.code_left()) + widest + px(TRAILING_SPACE)) * self.rows.columns() as f32
    }

    fn column_width(&self, bounds: Bounds<Pixels>) -> Pixels {
        bounds.size.width / self.rows.columns() as f32
    }

    fn column_left(&self, bounds: Bounds<Pixels>, column: usize) -> Pixels {
        bounds.origin.x + self.column_width(bounds) * column as f32
    }

    fn bounds_for_cell(&self, bounds: Bounds<Pixels>, cell: usize) -> Bounds<Pixels> {
        let columns = self.rows.columns();
        let code_left = px(self.rows.code_left());
        Bounds::new(
            point(
                self.column_left(bounds, cell % columns) + code_left,
                bounds.origin.y + px((cell / columns) as f32 * ROW_HEIGHT),
            ),
            size(
                (self.column_width(bounds) - code_left).max(px(0.)),
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

    fn visible_cells(&self) -> Range<usize> {
        let columns = self.rows.columns();
        self.visible.start * columns..self.visible.end * columns
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

        let columns = self.rows.columns();
        let visible = self.visible_cells();
        let cells = rows.start() * columns..(rows.end() + 1) * columns;
        let ranges: Vec<Option<Range<usize>>> = cells
            .clone()
            .map(|cell| {
                if visible.contains(&cell) {
                    return projected.get(cell - visible.start).and_then(Clone::clone);
                }
                let cell_bounds = self.bounds_for_cell(bounds, cell);
                let band = selection_band(cell_bounds.origin.y, anchor, cursor)?;
                let text = &self.strings[cell];
                selected_range(text, band, cell_bounds.origin.x, || {
                    pen.measure(text.clone(), window)
                })
            })
            .collect();

        copy_text(&self.strings[cells.clone()], &ranges, cells.start, columns)
    }

    fn paint_background(
        &self,
        row: usize,
        bounds: Bounds<Pixels>,
        top: Pixels,
        window: &mut Window,
    ) {
        let full_width = match &self.rows {
            Rows::Unified(rows) => Some(&rows[row]),
            Rows::Split(split) => match &split[row] {
                SplitRow::Full(full) => Some(full),
                SplitRow::Sides { .. } => None,
            },
        };

        match full_width {
            Some(full) => {
                if let Some(background) = row_background(full, &self.theme, self.mode) {
                    let band = Bounds::new(
                        point(bounds.origin.x, top),
                        size(bounds.size.width, px(ROW_HEIGHT)),
                    );
                    window.paint_quad(fill(band, background));
                }
            }
            None => {
                for column in 0..self.rows.columns() {
                    let Some(background) = self.rows.side(row, column).and_then(|side| {
                        line_colors(side.origin, self.mode, &self.theme).background
                    }) else {
                        continue;
                    };
                    let band = Bounds::new(
                        point(self.column_left(bounds, column), top),
                        size(self.column_width(bounds), px(ROW_HEIGHT)),
                    );
                    window.paint_quad(fill(band, background));
                }

                for column in 1..self.rows.columns() {
                    let rule = Bounds::new(
                        point(self.column_left(bounds, column), top),
                        size(px(COLUMN_RULE_WIDTH), px(ROW_HEIGHT)),
                    );
                    window.paint_quad(fill(rule, self.theme.border));
                }
            }
        }
    }

    fn paint_gutter(
        &self,
        row: usize,
        column: usize,
        cell_bounds: Bounds<Pixels>,
        pen: &Pen,
        window: &mut Window,
        cx: &mut App,
    ) {
        let left = cell_bounds.origin.x - px(self.rows.code_left());
        let top = cell_bounds.origin.y;
        let muted = self.theme.muted_foreground;
        let (origin, marker_left) = match &self.rows {
            Rows::Unified(rows) => {
                let Row::Line {
                    origin,
                    old_number,
                    new_number,
                    ..
                } = rows[row]
                else {
                    return;
                };
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
                (origin, left + px(2. * GUTTER_WIDTH + MARKER_PADDING))
            }
            Rows::Split(_) => {
                let Some(side) = self.rows.side(row, column) else {
                    return;
                };
                paint_number(
                    side.number,
                    left + px(GUTTER_WIDTH),
                    top,
                    muted,
                    pen,
                    window,
                    cx,
                );
                (side.origin, left + px(GUTTER_WIDTH + MARKER_PADDING))
            }
        };

        let foreground = line_colors(origin, self.mode, &self.theme).foreground;
        let line = pen.shape(marker(origin).into(), foreground, window);
        paint_line(&line, point(marker_left, top), window, cx);
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
            .visible_cells()
            .map(|cell| {
                styled_cell(
                    &self.rows,
                    cell,
                    self.strings[cell].clone(),
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
        self.cell_bounds = self
            .visible_cells()
            .map(|cell| self.bounds_for_cell(bounds, cell))
            .collect();
        for (text, cell_bounds) in self.texts.iter_mut().zip(&self.cell_bounds) {
            text.prepaint(None, None, *cell_bounds, &mut (), window, cx);
        }

        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        self.selection.register(
            TextSelectionRegistration::new(hitbox.clone(), bounds)
                .with_document_order(0)
                .with_text_bounds(self.cell_bounds.clone()),
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
        let first_cell = self.visible_cells().start;
        let runs: Vec<TextSelectionRun> = self
            .texts
            .iter()
            .enumerate()
            .map(|(offset, text)| {
                let cell = first_cell + offset;
                TextSelectionRun::new(
                    self.strings[cell].clone(),
                    text.layout().clone(),
                    self.cell_bounds[offset],
                )
                .with_document_order(cell as u64)
            })
            .collect();
        let projection = self.selection.update_runs(&runs, cx);

        let pen = Pen::new(window);
        let selected = self.copy_selection(bounds, projection.ranges(), &pen, window, cx);
        self.selection.set_fallback_copy_text(selected, cx);

        let columns = self.rows.columns();
        for (offset, row) in self.visible.clone().enumerate() {
            let top = self.cell_bounds[offset * columns].origin.y;
            self.paint_background(row, bounds, top, window);

            for column in 0..columns {
                let cell_offset = offset * columns + column;
                let cell_bounds = self.cell_bounds[cell_offset];
                if let Some(range) = projection.ranges().get(cell_offset).and_then(Clone::clone) {
                    paint_selection(
                        self.texts[cell_offset].layout(),
                        range,
                        self.theme.selection,
                        window,
                    );
                }

                self.paint_gutter(row, column, cell_bounds, &pen, window, cx);
                self.texts[cell_offset].paint(
                    None,
                    None,
                    cell_bounds,
                    &mut (),
                    &mut (),
                    window,
                    cx,
                );
            }
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
        assert_eq!(copy_text(&strings, &ranges, 0, 1), "ne\ntwo");
    }

    #[test]
    fn copy_text_keeps_a_blank_row_inside_the_selection() {
        let strings = vec![
            SharedString::from("one"),
            SharedString::from(""),
            SharedString::from("three"),
        ];
        let ranges = vec![Some(0..3), None, Some(0..5)];
        assert_eq!(copy_text(&strings, &ranges, 0, 1), "one\n\nthree");
    }

    #[test]
    fn copy_text_of_an_empty_projection_is_empty() {
        let strings = vec![SharedString::from("one")];
        assert_eq!(copy_text(&strings, &[None], 0, 1), "");
    }

    #[test]
    fn copy_text_separates_two_columns_of_the_same_row_with_a_tab() {
        let strings = vec![
            SharedString::from("gone"),
            SharedString::from("new"),
            SharedString::from("keep"),
            SharedString::from("keep"),
        ];
        let ranges = vec![Some(0..4), Some(0..3), Some(0..4), Some(0..4)];
        assert_eq!(copy_text(&strings, &ranges, 0, 2), "gone\tnew\nkeep\tkeep");
    }

    #[test]
    fn copy_text_of_a_padded_column_inside_the_selection_keeps_its_empty_field() {
        let strings = vec![
            SharedString::from("gone"),
            SharedString::from(""),
            SharedString::from("keep"),
            SharedString::from("keep"),
        ];
        let ranges = vec![Some(0..4), None, Some(0..4), Some(0..4)];
        assert_eq!(copy_text(&strings, &ranges, 0, 2), "gone\t\nkeep\tkeep");
    }

    #[test]
    fn copy_text_starts_at_the_first_selected_column_rather_than_at_a_leading_pad() {
        let strings = vec![SharedString::from(""), SharedString::from("new")];
        let ranges = vec![None, Some(0..3)];
        assert_eq!(copy_text(&strings, &ranges, 0, 2), "new");
    }

    #[test]
    fn copy_text_of_a_span_starting_mid_row_keeps_the_row_boundaries_aligned() {
        let strings = vec![
            SharedString::from("new"),
            SharedString::from("keep"),
            SharedString::from("keep"),
        ];
        let ranges = vec![Some(0..3), Some(0..4), Some(0..4)];
        assert_eq!(copy_text(&strings, &ranges, 1, 2), "new\nkeep\tkeep");
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
    fn the_window_starts_at_the_first_row_when_the_view_is_over_scrolled_upwards() {
        let window = row_window(px(60.), px(10. * ROW_HEIGHT), 500);
        assert_eq!(window.start, 0);
        assert_eq!(window.end, 12);
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
        assert_eq!(band, Some(Band::Between(px(20.), px(80.))));
    }

    #[test]
    fn the_first_row_of_a_selection_runs_from_its_endpoint_to_the_end_of_the_line() {
        let band = selection_band(px(36.), point(px(80.), px(40.)), point(px(20.), px(200.)));
        assert_eq!(band, Some(Band::From(px(80.))));
    }

    #[test]
    fn the_last_row_of_a_selection_runs_from_the_start_of_the_line_to_its_endpoint() {
        let band = selection_band(px(36.), point(px(80.), px(-10.)), point(px(20.), px(40.)));
        assert_eq!(band, Some(Band::To(px(20.))));
    }

    #[test]
    fn a_row_between_the_endpoints_is_selected_whole() {
        let band = selection_band(px(36.), point(px(80.), px(-10.)), point(px(20.), px(200.)));
        assert_eq!(band, Some(Band::Whole));
    }

    #[test]
    fn a_drag_upwards_bands_its_rows_exactly_as_the_same_drag_downwards() {
        let low = point(px(80.), px(40.));
        let high = point(px(20.), px(200.));
        let above = point(px(20.), px(-10.));

        assert_eq!(
            selection_band(px(36.), high, low),
            selection_band(px(36.), low, high)
        );
        assert_eq!(
            selection_band(px(36.), low, above),
            selection_band(px(36.), above, low)
        );
        assert_eq!(
            selection_band(px(36.), high, above),
            selection_band(px(36.), above, high)
        );
        assert_eq!(
            selection_band(px(36.), high, low),
            Some(Band::From(px(80.)))
        );
        assert_eq!(selection_band(px(36.), low, above), Some(Band::To(px(80.))));
        assert_eq!(selection_band(px(36.), high, above), Some(Band::Whole));
    }

    #[test]
    fn a_row_outside_the_selection_has_no_band() {
        let above = selection_band(px(0.), point(px(80.), px(40.)), point(px(20.), px(50.)));
        let below = selection_band(px(90.), point(px(80.), px(40.)), point(px(20.), px(50.)));
        assert_eq!(above, None);
        assert_eq!(below, None);
    }

    #[test]
    fn a_whole_row_needs_no_shaping_and_an_empty_one_selects_nothing() {
        let shape = || unreachable!("a whole row must not be shaped");
        assert_eq!(
            selected_range("one", Band::Whole, px(0.), shape),
            Some(0..3)
        );
        assert_eq!(selected_range("", Band::Whole, px(0.), shape), None);
    }
}
