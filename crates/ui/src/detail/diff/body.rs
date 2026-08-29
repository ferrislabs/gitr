use std::ops::{Range, RangeInclusive};
use std::rc::Rc;

use domain::LineOrigin;
use gpui::{
    App, Bounds, DispatchPhase, Element, ElementId, FlexDirection, GlobalElementId, Half as _,
    HighlightStyle, Hitbox, HitboxBehavior, Hsla, InspectorElementId, IntoElement, LayoutId,
    Length, MouseButton, MouseDownEvent, Pixels, Point, ScrollHandle, ShapedLine, SharedString,
    Style, StyledText, TextAlign, TextLayout, TextStyle, Window, fill, point, px, relative, size,
};
use gpui_base::{TextSelectionHandle, TextSelectionRegistration, TextSelectionRun};
use gpui_component::{ThemeColor, ThemeMode};

use super::model::{Row, header_line};
use super::pairing::SideLine;
use super::palette::line_colors;
use super::split::SplitRow;
use super::{DiffContent, ToggleFile};

pub(super) const ROW_HEIGHT: f32 = 18.;

const GUTTER_WIDTH: f32 = 44.;
const MARKER_WIDTH: f32 = 16.;
const GUTTER_PADDING: f32 = 8.;
const MARKER_PADDING: f32 = 5.;
const CODE_LEFT: f32 = 2. * GUTTER_WIDTH + MARKER_WIDTH;
const SPLIT_CODE_LEFT: f32 = GUTTER_WIDTH + MARKER_WIDTH;
const COLUMN_RULE_WIDTH: f32 = 1.;
const TRAILING_SPACE: f32 = 16.;
const UNMEASURED_ROWS: usize = 100;

pub(super) enum Rows {
    Unified(Vec<Row>),
    Split(Vec<SplitRow>),
}

impl Rows {
    pub(super) fn len(&self) -> usize {
        match self {
            Rows::Unified(rows) => rows.len(),
            Rows::Split(rows) => rows.len(),
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.len() == 0
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

    fn marker_left(&self) -> f32 {
        self.code_left() - MARKER_WIDTH + MARKER_PADDING
    }

    fn full(&self, row: usize) -> Option<&Row> {
        match self {
            Rows::Unified(rows) => rows.get(row),
            Rows::Split(rows) => match rows.get(row)? {
                SplitRow::Full(full) => Some(full),
                SplitRow::Sides { .. } => None,
            },
        }
    }

    fn file_at(&self, row: usize) -> Option<usize> {
        match self.full(row)? {
            Row::FileHeader { index, .. } => Some(*index),
            _ => None,
        }
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

pub(super) fn cell_strings(rows: &Rows) -> Vec<SharedString> {
    (0..rows.cells())
        .map(|cell| cell_text(rows, cell))
        .collect()
}

pub(super) struct DiffBody {
    content: Rc<DiffContent>,
    select_all: bool,
    selection: TextSelectionHandle,
    scroll: ScrollHandle,
    toggle_file: ToggleFile,
    theme: ThemeColor,
    mode: ThemeMode,
    visible: Range<usize>,
    texts: Vec<StyledText>,
    cell_bounds: Vec<Bounds<Pixels>>,
}

pub(super) fn body(
    content: Rc<DiffContent>,
    select_all: bool,
    selection: TextSelectionHandle,
    scroll: ScrollHandle,
    toggle_file: ToggleFile,
    theme: ThemeColor,
    mode: ThemeMode,
) -> DiffBody {
    DiffBody {
        content,
        select_all,
        selection,
        scroll,
        toggle_file,
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
        Row::FileHeader {
            path,
            status,
            added,
            deleted,
            ..
        } => header_line(path, status, *added, *deleted).into(),
        Row::Separator => SharedString::default(),
        Row::Line { content, .. } => content.clone().into(),
        Row::Placeholder { message } => (*message).into(),
    }
}

fn styled_cell(rows: &Rows, cell: usize, text: SharedString, theme: &ThemeColor) -> StyledText {
    let range = 0..text.len();
    let highlight = HighlightStyle {
        color: Some(cell_foreground(rows, cell, theme)),
        ..Default::default()
    };
    StyledText::new(text).with_highlights([(range, highlight)])
}

fn cell_foreground(rows: &Rows, cell: usize, theme: &ThemeColor) -> Hsla {
    rows.full(cell / rows.columns())
        .map_or(theme.foreground, |full| row_foreground(full, theme))
}

fn row_foreground(row: &Row, theme: &ThemeColor) -> Hsla {
    match row {
        Row::FileHeader { .. } | Row::Line { .. } => theme.foreground,
        Row::Separator | Row::Placeholder { .. } => theme.muted_foreground,
    }
}

fn row_background(row: &Row, theme: &ThemeColor, mode: ThemeMode) -> Option<Hsla> {
    match row {
        Row::FileHeader { .. } => Some(theme.secondary),
        Row::Separator => Some(theme.muted),
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

fn disclosure(collapsed: bool) -> &'static str {
    if collapsed { "\u{25b8}" } else { "\u{25be}" }
}

fn column_width(bounds: Bounds<Pixels>, columns: usize) -> Pixels {
    bounds.size.width / columns as f32
}

fn column_left(bounds: Bounds<Pixels>, columns: usize, column: usize) -> Pixels {
    bounds.origin.x + column_width(bounds, columns) * column as f32
}

fn bounds_for_cell(
    bounds: Bounds<Pixels>,
    columns: usize,
    code_left: Pixels,
    cell: usize,
) -> Bounds<Pixels> {
    Bounds::new(
        point(
            column_left(bounds, columns, cell % columns) + code_left,
            bounds.origin.y + px((cell / columns) as f32 * ROW_HEIGHT),
        ),
        size(
            (column_width(bounds, columns) - code_left).max(px(0.)),
            px(ROW_HEIGHT),
        ),
    )
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
        return 0..rows.min(UNMEASURED_ROWS);
    }
    let first = ((-offset_y) / px(ROW_HEIGHT)).floor().max(0.) as usize;
    let count = (viewport / px(ROW_HEIGHT)).ceil() as usize + 2;
    first.min(rows)..first.saturating_add(count).min(rows)
}

fn row_at(origin_y: Pixels, y: Pixels, rows: usize) -> Option<usize> {
    let row = ((y - origin_y) / px(ROW_HEIGHT)).floor();
    (row >= 0. && row < rows as f32).then_some(row as usize)
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
    fn rows(&self) -> &Rows {
        &self.content.rows
    }

    fn strings(&self) -> &[SharedString] {
        &self.content.strings
    }

    fn cell_bounds_at(&self, bounds: Bounds<Pixels>, cell: usize) -> Bounds<Pixels> {
        bounds_for_cell(
            bounds,
            self.rows().columns(),
            px(self.rows().code_left()),
            cell,
        )
    }

    fn content_width(&self, window: &Window) -> Pixels {
        let pen = Pen::new(window);
        let mut widest = px(0.);
        for text in self.strings() {
            widest = widest.max(pen.width(text.clone(), window));
        }
        (px(self.rows().code_left()) + widest + px(TRAILING_SPACE)) * self.rows().columns() as f32
    }

    fn visible_rows(&self) -> Range<usize> {
        row_window(
            self.scroll.offset().y,
            self.scroll.bounds().size.height,
            self.rows().len(),
        )
    }

    fn visible_cells(&self) -> Range<usize> {
        let columns = self.rows().columns();
        self.visible.start * columns..self.visible.end * columns
    }

    fn whole_text(&self) -> String {
        let ranges: Vec<Option<Range<usize>>> = self
            .strings()
            .iter()
            .map(|text| Some(0..text.len()))
            .collect();
        copy_text(self.strings(), &ranges, 0, self.rows().columns())
    }

    fn copy_selection(
        &self,
        bounds: Bounds<Pixels>,
        projected: &[Option<Range<usize>>],
        pen: &Pen,
        window: &Window,
        cx: &App,
    ) -> String {
        if self.select_all {
            return self.whole_text();
        }
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
            self.rows().len(),
        ) else {
            return String::new();
        };

        let columns = self.rows().columns();
        let visible = self.visible_cells();
        let cells = rows.start() * columns..(rows.end() + 1) * columns;
        let ranges: Vec<Option<Range<usize>>> = cells
            .clone()
            .map(|cell| {
                if visible.contains(&cell) {
                    return projected.get(cell - visible.start).and_then(Clone::clone);
                }
                let cell_bounds = self.cell_bounds_at(bounds, cell);
                let band = selection_band(cell_bounds.origin.y, anchor, cursor)?;
                let text = &self.strings()[cell];
                selected_range(text, band, cell_bounds.origin.x, || {
                    pen.measure(text.clone(), window)
                })
            })
            .collect();

        copy_text(
            &self.strings()[cells.clone()],
            &ranges,
            cells.start,
            columns,
        )
    }

    fn paint_background(
        &self,
        row: usize,
        bounds: Bounds<Pixels>,
        top: Pixels,
        window: &mut Window,
    ) {
        let columns = self.rows().columns();
        match self.rows().full(row) {
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
                for column in 0..columns {
                    let Some(background) = self.rows().side(row, column).and_then(|side| {
                        line_colors(side.origin, self.mode, &self.theme).background
                    }) else {
                        continue;
                    };
                    let band = Bounds::new(
                        point(column_left(bounds, columns, column), top),
                        size(column_width(bounds, columns), px(ROW_HEIGHT)),
                    );
                    window.paint_quad(fill(band, background));
                }

                for column in 1..columns {
                    let rule = Bounds::new(
                        point(column_left(bounds, columns, column), top),
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
        let left = cell_bounds.origin.x - px(self.rows().code_left());
        let top = cell_bounds.origin.y;
        let marker_left = left + px(self.rows().marker_left());
        let muted = self.theme.muted_foreground;

        if let Some(header @ Row::FileHeader { collapsed, .. }) = self.rows().full(row) {
            if column == 0 {
                let color = row_foreground(header, &self.theme);
                let line = pen.shape(disclosure(*collapsed).into(), color, window);
                paint_line(&line, point(marker_left, top), window, cx);
            }
            return;
        }

        let origin = match self.rows() {
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
                origin
            }
            Rows::Split(_) => {
                let Some(side) = self.rows().side(row, column) else {
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
                side.origin
            }
        };

        let marker_color = line_colors(origin, self.mode, &self.theme).foreground;
        let line = pen.shape(marker(origin).into(), marker_color, window);
        paint_line(&line, point(marker_left, top), window, cx);
    }

    fn on_mouse_down(&self, bounds: Bounds<Pixels>, hitbox: &Hitbox, window: &mut Window) {
        let hitbox = hitbox.clone();
        let content = Rc::clone(&self.content);
        let toggle_file = Rc::clone(&self.toggle_file);
        let origin_y = bounds.origin.y;
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble
                || event.button != MouseButton::Left
                || event.click_count != 1
                || !hitbox.is_hovered(window)
            {
                return;
            }
            let Some(row) = row_at(origin_y, event.position.y, content.rows.len()) else {
                return;
            };
            if let Some(file) = content.rows.file_at(row) {
                toggle_file(&file, window, cx);
            }
        });
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
                    self.rows(),
                    cell,
                    self.content.strings[cell].clone(),
                    &self.theme,
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
            size: size(
                width.into(),
                px(self.rows().len() as f32 * ROW_HEIGHT).into(),
            ),
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
            .map(|cell| self.cell_bounds_at(bounds, cell))
            .collect();
        for (text, cell_bounds) in self.texts.iter_mut().zip(&self.cell_bounds) {
            text.prepaint(None, None, *cell_bounds, &mut (), window, cx);
        }

        let viewport = self.scroll.bounds();
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        self.selection.register(
            TextSelectionRegistration::new(hitbox.clone(), viewport)
                .with_scroll_offset(bounds.origin - viewport.origin)
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
        hitbox: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.on_mouse_down(bounds, hitbox, window);

        let first_cell = self.visible_cells().start;
        let runs: Vec<TextSelectionRun> = self
            .texts
            .iter()
            .enumerate()
            .map(|(offset, text)| {
                let cell = first_cell + offset;
                TextSelectionRun::new(
                    self.content.strings[cell].clone(),
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

        let columns = self.rows().columns();
        for (offset, row) in self.visible.clone().enumerate() {
            let top = self.cell_bounds[offset * columns].origin.y;
            self.paint_background(row, bounds, top, window);

            for column in 0..columns {
                let cell_offset = offset * columns + column;
                let cell_bounds = self.cell_bounds[cell_offset];
                let range = if self.select_all {
                    Some(0..self.content.strings[first_cell + cell_offset].len())
                } else {
                    projection.ranges().get(cell_offset).and_then(Clone::clone)
                };
                if let Some(range) = range {
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
    use domain::FileStatus;

    fn file_header() -> Row {
        Row::FileHeader {
            index: 0,
            path: "src/main.rs".to_string(),
            status: FileStatus::Modified,
            added: 3,
            deleted: 1,
            collapsed: false,
        }
    }

    fn line(origin: LineOrigin, content: &str) -> Row {
        Row::Line {
            origin,
            old_number: Some(1),
            new_number: Some(1),
            content: content.to_string(),
        }
    }

    fn side(origin: LineOrigin, content: &str) -> SideLine {
        SideLine {
            number: Some(1),
            origin,
            content: content.to_string(),
        }
    }

    fn split_rows() -> Rows {
        Rows::Split(vec![
            SplitRow::Full(file_header()),
            SplitRow::Sides {
                left: Some(side(LineOrigin::Deletion, "gone")),
                right: None,
            },
        ])
    }

    #[test]
    fn a_unified_view_has_one_column_and_a_split_view_two() {
        let unified = Rows::Unified(vec![file_header(), line(LineOrigin::Context, "keep")]);
        let split = split_rows();

        assert_eq!(unified.columns(), 1);
        assert_eq!(unified.cells(), 2);
        assert_eq!(unified.code_left(), CODE_LEFT);

        assert_eq!(split.columns(), 2);
        assert_eq!(split.cells(), 4);
        assert_eq!(split.code_left(), SPLIT_CODE_LEFT);
    }

    #[test]
    fn a_marker_sits_in_the_last_gutter_a_view_has() {
        let unified = Rows::Unified(vec![file_header()]);

        assert_eq!(unified.marker_left(), 2. * GUTTER_WIDTH + MARKER_PADDING);
        assert_eq!(split_rows().marker_left(), GUTTER_WIDTH + MARKER_PADDING);
    }

    #[test]
    fn an_empty_row_list_is_empty_in_either_view() {
        assert!(Rows::Unified(Vec::new()).is_empty());
        assert!(Rows::Split(Vec::new()).is_empty());
        assert!(!Rows::Unified(vec![file_header()]).is_empty());
    }

    #[test]
    fn only_a_split_row_with_two_sides_has_a_side() {
        let rows = split_rows();

        assert_eq!(rows.side(0, 0), None, "a full-width row has no side");
        assert_eq!(
            rows.side(1, 0).map(|side| side.content.as_str()),
            Some("gone")
        );
        assert_eq!(rows.side(1, 1), None, "the padded side is absent");
        assert_eq!(
            Rows::Unified(vec![file_header()]).side(0, 0),
            None,
            "a unified row has no sides at all"
        );
    }

    #[test]
    fn only_a_file_header_row_names_a_file() {
        let unified = Rows::Unified(vec![
            file_header(),
            Row::Separator,
            line(LineOrigin::Context, "keep"),
        ]);

        assert_eq!(unified.file_at(0), Some(0));
        assert_eq!(unified.file_at(1), None);
        assert_eq!(unified.file_at(2), None);
        assert_eq!(unified.file_at(3), None, "there is no fourth row");
        assert_eq!(split_rows().file_at(0), Some(0));
        assert_eq!(
            split_rows().file_at(1),
            None,
            "a two-sided row is never a header"
        );
    }

    #[test]
    fn a_row_renders_its_own_kind_of_text() {
        assert_eq!(
            row_text(&file_header()),
            SharedString::from("src/main.rs  +3 \u{2212}1")
        );
        assert_eq!(row_text(&Row::Separator), SharedString::default());
        assert_eq!(
            row_text(&line(LineOrigin::Addition, "let x = 1;")),
            SharedString::from("let x = 1;")
        );
        assert_eq!(
            row_text(&Row::Placeholder {
                message: "Binary file not shown."
            }),
            SharedString::from("Binary file not shown.")
        );
    }

    #[test]
    fn a_unified_cell_is_its_row() {
        let rows = Rows::Unified(vec![file_header(), line(LineOrigin::Deletion, "gone")]);
        assert_eq!(
            cell_text(&rows, 0),
            SharedString::from("src/main.rs  +3 \u{2212}1")
        );
        assert_eq!(cell_text(&rows, 1), SharedString::from("gone"));
    }

    #[test]
    fn a_full_width_split_row_renders_in_the_first_column_and_blank_in_the_second() {
        let rows = split_rows();
        assert_eq!(
            cell_text(&rows, 0),
            SharedString::from("src/main.rs  +3 \u{2212}1")
        );
        assert_eq!(cell_text(&rows, 1), SharedString::default());
    }

    #[test]
    fn a_separator_carries_no_text_in_either_view() {
        let unified = Rows::Unified(vec![Row::Separator]);
        let split = Rows::Split(vec![SplitRow::Full(Row::Separator)]);

        assert_eq!(cell_text(&unified, 0), SharedString::default());
        assert_eq!(cell_text(&split, 0), SharedString::default());
        assert_eq!(cell_text(&split, 1), SharedString::default());
    }

    #[test]
    fn a_disclosure_marker_turns_sideways_when_its_file_is_collapsed() {
        assert_eq!(disclosure(false), "\u{25be}");
        assert_eq!(disclosure(true), "\u{25b8}");
    }

    #[test]
    fn a_collapsed_header_reads_exactly_as_an_expanded_one() {
        let collapsed = Row::FileHeader {
            index: 0,
            path: "src/main.rs".to_string(),
            status: FileStatus::Modified,
            added: 3,
            deleted: 1,
            collapsed: true,
        };

        assert_eq!(
            row_text(&collapsed),
            row_text(&file_header()),
            "the marker is painted in the gutter, so no run and no copy can carry it"
        );
    }

    #[test]
    fn a_split_row_puts_each_side_in_its_own_column_and_pads_the_missing_one() {
        let rows = split_rows();
        assert_eq!(cell_text(&rows, 2), SharedString::from("gone"));
        assert_eq!(cell_text(&rows, 3), SharedString::default());
    }

    #[test]
    fn a_marker_names_the_origin_and_a_context_line_keeps_the_column_wide() {
        assert_eq!(marker(LineOrigin::Addition), "+");
        assert_eq!(marker(LineOrigin::Deletion), "\u{2212}");
        assert_eq!(marker(LineOrigin::Context), " ");
    }

    #[test]
    fn only_a_changed_line_the_file_header_and_a_separator_are_banded() {
        let theme = ThemeColor::light();
        let mode = ThemeMode::Light;

        assert_eq!(
            row_background(&file_header(), &theme, mode),
            Some(theme.secondary)
        );
        assert_eq!(
            row_background(&Row::Separator, &theme, mode),
            Some(theme.muted)
        );
        assert_eq!(
            row_background(
                &Row::Placeholder {
                    message: "No content changes."
                },
                &theme,
                mode
            ),
            None
        );
        assert_eq!(
            row_background(&line(LineOrigin::Context, "keep"), &theme, mode),
            None
        );
        assert_eq!(
            row_background(&line(LineOrigin::Addition, "new"), &theme, mode),
            line_colors(LineOrigin::Addition, mode, &theme).background
        );
    }

    #[test]
    fn a_column_takes_an_equal_share_of_the_width_from_left_to_right() {
        let bounds = Bounds::new(point(px(10.), px(20.)), size(px(200.), px(400.)));

        assert_eq!(column_width(bounds, 1), px(200.));
        assert_eq!(column_width(bounds, 2), px(100.));
        assert_eq!(column_left(bounds, 2, 0), px(10.));
        assert_eq!(column_left(bounds, 2, 1), px(110.));
    }

    #[test]
    fn a_cell_sits_at_its_column_past_the_gutters_and_at_its_row() {
        let bounds = Bounds::new(point(px(10.), px(20.)), size(px(200.), px(400.)));

        assert_eq!(
            bounds_for_cell(bounds, 2, px(60.), 3),
            Bounds::new(
                point(px(170.), px(20. + ROW_HEIGHT)),
                size(px(40.), px(ROW_HEIGHT))
            ),
            "cell 3 is the right column of the second row"
        );
        assert_eq!(
            bounds_for_cell(bounds, 1, px(104.), 0),
            Bounds::new(point(px(114.), px(20.)), size(px(96.), px(ROW_HEIGHT)))
        );
    }

    #[test]
    fn a_column_narrower_than_its_gutters_leaves_no_width_rather_than_a_negative_one() {
        let bounds = Bounds::new(point(px(0.), px(0.)), size(px(80.), px(400.)));
        assert_eq!(bounds_for_cell(bounds, 2, px(60.), 0).size.width, px(0.));
    }

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
    fn the_window_is_one_screenful_until_the_viewport_has_been_measured() {
        assert_eq!(row_window(px(0.), px(0.), 500), 0..UNMEASURED_ROWS);
        assert_eq!(row_window(px(0.), px(0.), 12), 0..12);
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
    fn a_click_lands_on_the_row_its_height_puts_it_in() {
        assert_eq!(row_at(px(100.), px(100.), 3), Some(0));
        assert_eq!(row_at(px(100.), px(100. + ROW_HEIGHT - 1.), 3), Some(0));
        assert_eq!(row_at(px(100.), px(100. + ROW_HEIGHT), 3), Some(1));
        assert_eq!(row_at(px(100.), px(100. + 2.5 * ROW_HEIGHT), 3), Some(2));
    }

    #[test]
    fn a_click_outside_the_rows_lands_on_none_of_them() {
        assert_eq!(row_at(px(100.), px(99.), 3), None);
        assert_eq!(row_at(px(100.), px(100. + 3. * ROW_HEIGHT), 3), None);
        assert_eq!(row_at(px(100.), px(100.), 0), None);
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
