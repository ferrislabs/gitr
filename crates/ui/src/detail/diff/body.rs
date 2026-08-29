use std::ops::{Range, RangeInclusive};
use std::rc::Rc;

use domain::FileStatus;
use gpui::{
    App, Bounds, DispatchPhase, Element, ElementId, FlexDirection, FontWeight, GlobalElementId,
    Half as _, HighlightStyle, Hitbox, HitboxBehavior, Hsla, InspectorElementId, IntoElement,
    LayoutId, Length, MouseButton, MouseDownEvent, Pixels, Point, ScrollHandle, ShapedLine,
    SharedString, Style, StyledText, TextAlign, TextLayout, TextStyle, Window, fill, point, px,
    relative, size,
};
use gpui_base::{TextSelectionHandle, TextSelectionRegistration, TextSelectionRun};
use gpui_component::{ThemeColor, ThemeMode, scroll::Scrollbar};

use super::model::Row;
use super::pairing::SideLine;
use super::palette::line_background;
use super::split::SplitRow;
use super::{DiffContent, ToggleFile};

pub(super) const ROW_HEIGHT: f32 = 18.;

const GUTTER_WIDTH: f32 = 44.;
const GUTTER_PADDING: f32 = 8.;
const CODE_LEFT: f32 = 2. * GUTTER_WIDTH;
const SPLIT_CODE_LEFT: f32 = GUTTER_WIDTH;
const COLUMN_RULE_WIDTH: f32 = 1.;
const TRAILING_SPACE: f32 = 16.;
const UNMEASURED_ROWS: usize = 100;

const HEADER_PADDING: f32 = 8.;
const DISCLOSURE_WIDTH: f32 = 12.;
const PASTILLE_SIZE: f32 = 8.;
const PASTILLE_RADIUS: f32 = 2.;
const PASTILLE_GAP: f32 = 8.;
const PASTILLE_LEFT: f32 = HEADER_PADDING + DISCLOSURE_WIDTH;
const PASTILLE_TOP: f32 = (ROW_HEIGHT - PASTILLE_SIZE) / 2.;
const HEADER_TEXT_LEFT: f32 = PASTILLE_LEFT + PASTILLE_SIZE + PASTILLE_GAP;
const BAR_WIDTH: f32 = 64.;
const BAR_HEIGHT: f32 = 8.;
const BAR_TOP: f32 = (ROW_HEIGHT - BAR_HEIGHT) / 2.;
const BAR_MIN_SEGMENT: f32 = 2.;
const BAR_MIN_WIDTH: f32 = 2. * BAR_MIN_SEGMENT;
const BAR_GAP: f32 = 8.;
const COUNT_WIDTH: f32 = 34.;
const HEADER_STATS_WIDTH: f32 = BAR_GAP + BAR_WIDTH + BAR_GAP + COUNT_WIDTH + HEADER_PADDING;
const ELLIPSIS: &str = "\u{2026}";

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

    fn is_header(&self, row: usize) -> bool {
        matches!(self.full(row), Some(Row::FileHeader { .. }))
    }

    fn cell_left(&self, row: usize) -> f32 {
        if self.is_header(row) {
            HEADER_TEXT_LEFT
        } else {
            self.code_left()
        }
    }

    fn cells(&self) -> usize {
        self.len() * self.columns()
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
    path_budget: Option<Pixels>,
    display: Vec<SharedString>,
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
        path_budget: None,
        display: Vec::new(),
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
        Row::FileHeader { path, .. } => path.clone().into(),
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
        Row::Line { origin, .. } => line_background(*origin, mode),
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
    left: Pixels,
    cell: usize,
) -> Bounds<Pixels> {
    Bounds::new(
        point(
            column_left(bounds, columns, cell % columns) + left,
            bounds.origin.y + px((cell / columns) as f32 * ROW_HEIGHT),
        ),
        size(
            (column_width(bounds, columns) - left).max(px(0.)),
            px(ROW_HEIGHT),
        ),
    )
}

fn header_budget(viewport: Pixels) -> Option<Pixels> {
    if viewport <= px(0.) {
        return None;
    }
    let furniture = px(HEADER_TEXT_LEFT + HEADER_STATS_WIDTH) + Scrollbar::width();
    Some((viewport - furniture).max(px(0.)))
}

fn count_right_edge(frame_right: Pixels) -> Pixels {
    frame_right - Scrollbar::width() - px(HEADER_PADDING)
}

fn bar_right_edge(frame_right: Pixels, count_width: Pixels) -> Pixels {
    count_right_edge(frame_right) - px(BAR_GAP) - px(COUNT_WIDTH).max(count_width)
}

fn elide_path(path: &str, budget: f32, width: impl Fn(&str) -> f32) -> Option<String> {
    if width(path) <= budget {
        return None;
    }
    let tails: Vec<usize> = path
        .char_indices()
        .map(|(offset, _)| offset)
        .skip(1)
        .chain([path.len()])
        .collect();
    let elided = |tail: usize| format!("{ELLIPSIS}{}", &path[tail..]);
    let index = tails.partition_point(|tail| width(&elided(*tail)) > budget);
    Some(
        tails
            .get(index)
            .map_or_else(|| ELLIPSIS.to_string(), |tail| elided(*tail)),
    )
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Bar {
    added: f32,
    deleted: f32,
}

fn bar_widths(added: usize, deleted: usize, max_changes: usize) -> Option<Bar> {
    let total = added + deleted;
    if total == 0 || max_changes == 0 {
        return None;
    }
    let width = (BAR_WIDTH * total as f32 / max_changes as f32).max(BAR_MIN_WIDTH);
    let mut green = width * added as f32 / total as f32;
    if added > 0 {
        green = green.max(BAR_MIN_SEGMENT);
    }
    if deleted > 0 {
        green = green.min(width - BAR_MIN_SEGMENT);
    }
    Some(Bar {
        added: green,
        deleted: width - green,
    })
}

fn status_color(status: &FileStatus, theme: &ThemeColor) -> Hsla {
    match status {
        FileStatus::Added => theme.green,
        FileStatus::Deleted => theme.red,
        FileStatus::Modified => theme.blue,
        FileStatus::Renamed { .. } | FileStatus::Copied { .. } | FileStatus::TypeChanged => {
            theme.muted_foreground
        }
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
        self.weighted(text, color, self.style.font_weight, window)
    }

    fn bold(&self, text: SharedString, color: Hsla, window: &Window) -> ShapedLine {
        self.weighted(text, color, FontWeight::BOLD, window)
    }

    fn weighted(
        &self,
        text: SharedString,
        color: Hsla,
        weight: FontWeight,
        window: &Window,
    ) -> ShapedLine {
        let mut run = self.style.to_run(text.len());
        run.color = color;
        run.font.weight = weight;
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

    fn header_frame(&self, bounds: Bounds<Pixels>) -> Bounds<Pixels> {
        if self.path_budget.is_some() {
            self.scroll.bounds()
        } else {
            bounds
        }
    }

    fn cell_string(&self, cell: usize, pen: &Pen, window: &Window) -> SharedString {
        let text = self.content.strings[cell].clone();
        let columns = self.rows().columns();
        let Some(budget) = self
            .path_budget
            .filter(|_| self.rows().is_header(cell / columns))
        else {
            return text;
        };
        elide_path(&text, f32::from(budget), |candidate| {
            f32::from(pen.width(candidate.to_string().into(), window))
        })
        .map_or(text, SharedString::from)
    }

    fn cell_bounds_at(&self, bounds: Bounds<Pixels>, cell: usize) -> Bounds<Pixels> {
        let columns = self.rows().columns();
        let row = cell / columns;
        let Some(budget) = self.path_budget.filter(|_| self.rows().is_header(row)) else {
            return bounds_for_cell(bounds, columns, px(self.rows().cell_left(row)), cell);
        };
        Bounds::new(
            point(
                self.header_frame(bounds).origin.x + px(HEADER_TEXT_LEFT),
                bounds.origin.y + px(row as f32 * ROW_HEIGHT),
            ),
            size(budget, px(ROW_HEIGHT)),
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

    fn whole_text(&self, pen: &Pen, window: &Window) -> String {
        let texts: Vec<SharedString> = (0..self.rows().cells())
            .map(|cell| self.cell_string(cell, pen, window))
            .collect();
        let ranges: Vec<Option<Range<usize>>> =
            texts.iter().map(|text| Some(0..text.len())).collect();
        copy_text(&texts, &ranges, 0, self.rows().columns())
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
            return self.whole_text(pen, window);
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
        let displayed = |cell: usize| {
            visible
                .contains(&cell)
                .then(|| self.display.get(cell - visible.start))
                .flatten()
        };
        let texts: Vec<SharedString> = cells
            .clone()
            .map(|cell| {
                displayed(cell)
                    .cloned()
                    .unwrap_or_else(|| self.cell_string(cell, pen, window))
            })
            .collect();
        let ranges: Vec<Option<Range<usize>>> = cells
            .clone()
            .enumerate()
            .map(|(offset, cell)| {
                if displayed(cell).is_some() {
                    return projected.get(cell - visible.start).and_then(Clone::clone);
                }
                let cell_bounds = self.cell_bounds_at(bounds, cell);
                let band = selection_band(cell_bounds.origin.y, anchor, cursor)?;
                let text = &texts[offset];
                selected_range(text, band, cell_bounds.origin.x, || {
                    pen.measure(text.clone(), window)
                })
            })
            .collect();

        copy_text(&texts, &ranges, cells.start, columns)
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
                    let Some(background) = self
                        .rows()
                        .side(row, column)
                        .and_then(|side| line_background(side.origin, self.mode))
                    else {
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
        let muted = self.theme.muted_foreground;

        match self.rows() {
            Rows::Unified(rows) => {
                let Row::Line {
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
            }
        }
    }

    fn paint_header(
        &self,
        header: &Row,
        bounds: Bounds<Pixels>,
        top: Pixels,
        pen: &Pen,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Row::FileHeader {
            status,
            added,
            deleted,
            collapsed,
            ..
        } = header
        else {
            return;
        };

        let frame = self.header_frame(bounds);
        let chevron = pen.shape(
            disclosure(*collapsed).into(),
            self.theme.muted_foreground,
            window,
        );
        paint_line(
            &chevron,
            point(frame.origin.x + px(HEADER_PADDING), top),
            window,
            cx,
        );

        window.paint_quad(
            fill(
                Bounds::new(
                    point(frame.origin.x + px(PASTILLE_LEFT), top + px(PASTILLE_TOP)),
                    size(px(PASTILLE_SIZE), px(PASTILLE_SIZE)),
                ),
                status_color(status, &self.theme),
            )
            .corner_radii(px(PASTILLE_RADIUS)),
        );

        let count = pen.bold(
            (added + deleted).to_string().into(),
            self.theme.foreground,
            window,
        );
        let count_right = count_right_edge(frame.right());
        paint_line(&count, point(count_right - count.width(), top), window, cx);

        let Some(bar) = bar_widths(*added, *deleted, self.content.max_changes) else {
            return;
        };
        let bar_right = bar_right_edge(frame.right(), count.width());
        let bar_left = bar_right - px(bar.added + bar.deleted);
        let bar_top = top + px(BAR_TOP);
        for (offset, width, color) in [
            (0., bar.added, self.theme.green),
            (bar.added, bar.deleted, self.theme.red),
        ] {
            if width <= 0. {
                continue;
            }
            window.paint_quad(fill(
                Bounds::new(
                    point(bar_left + px(offset), bar_top),
                    size(px(width), px(BAR_HEIGHT)),
                ),
                color,
            ));
        }
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
        self.path_budget = header_budget(self.scroll.bounds().size.width);

        let pen = Pen::new(window);
        let display: Vec<SharedString> = self
            .visible_cells()
            .map(|cell| self.cell_string(cell, &pen, window))
            .collect();
        self.display = display;
        self.texts = self
            .visible_cells()
            .zip(&self.display)
            .map(|(cell, text)| styled_cell(self.rows(), cell, text.clone(), &self.theme))
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
                    self.display[offset].clone(),
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
                    Some(0..self.display[cell_offset].len())
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

                match self.rows().full(row) {
                    Some(header @ Row::FileHeader { .. }) if column == 0 => {
                        self.paint_header(header, bounds, top, &pen, window, cx);
                    }
                    Some(Row::FileHeader { .. }) => {}
                    _ => self.paint_gutter(row, column, cell_bounds, &pen, window, cx),
                }
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
    use domain::LineOrigin;

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
    fn code_starts_immediately_after_the_gutters_a_view_has() {
        let unified = Rows::Unified(vec![file_header()]);

        assert_eq!(unified.code_left(), 2. * GUTTER_WIDTH);
        assert_eq!(split_rows().code_left(), GUTTER_WIDTH);
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
        assert_eq!(row_text(&file_header()), SharedString::from("src/main.rs"));
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
        assert_eq!(cell_text(&rows, 0), SharedString::from("src/main.rs"));
        assert_eq!(cell_text(&rows, 1), SharedString::from("gone"));
    }

    #[test]
    fn a_full_width_split_row_renders_in_the_first_column_and_blank_in_the_second() {
        let rows = split_rows();
        assert_eq!(cell_text(&rows, 0), SharedString::from("src/main.rs"));
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
    fn a_header_starts_at_the_left_edge_and_every_other_row_past_its_gutters() {
        let unified = Rows::Unified(vec![file_header(), line(LineOrigin::Context, "keep")]);
        let split = split_rows();

        assert_eq!(unified.cell_left(0), HEADER_TEXT_LEFT);
        assert_eq!(unified.cell_left(1), CODE_LEFT);
        assert_eq!(split.cell_left(0), HEADER_TEXT_LEFT);
        assert_eq!(split.cell_left(1), SPLIT_CODE_LEFT);
        assert!(
            split.cell_left(0) < split.cell_left(1),
            "a header is flush left, ahead of the narrowest code column"
        );
    }

    fn monospace(text: &str) -> f32 {
        text.chars().count() as f32
    }

    #[test]
    fn a_path_that_fits_its_budget_is_left_alone() {
        assert_eq!(elide_path("src/main.rs", 40., monospace), None);
        assert_eq!(
            elide_path("0123456789", 10., monospace),
            None,
            "a path exactly as wide as its budget still fits"
        );
        assert_eq!(elide_path("", 0., monospace), None);
    }

    #[test]
    fn a_path_over_its_budget_loses_its_head_rather_than_its_tail() {
        assert_eq!(
            elide_path("crates/ui/src/detail.rs", 10., monospace),
            Some("\u{2026}detail.rs".to_string()),
            "the tail identifies the file, so the ellipsis leads"
        );
        assert_eq!(
            elide_path("0123456789", 9.9, monospace),
            Some("\u{2026}23456789".to_string()),
            "one character over budget drops two, since the ellipsis takes one"
        );
        assert_eq!(
            elide_path("d\u{e9}tail.rs", 5., monospace),
            Some("\u{2026}l.rs".to_string()),
            "the cut is a byte offset that lands on a character boundary, not a char count"
        );
    }

    #[test]
    fn an_elided_path_never_exceeds_the_budget_it_was_given() {
        for path in [
            "crates/ui/src/detail/diff/body.rs",
            "crates/ui/src/d\u{e9}tail/diff/b\u{f4}dy.rs",
        ] {
            for budget in 2..=40 {
                let budget = budget as f32;
                let elided =
                    elide_path(path, budget, monospace).unwrap_or_else(|| path.to_string());
                assert!(
                    monospace(&elided) <= budget,
                    "{elided:?} overruns a budget of {budget}"
                );
                assert!(
                    path.ends_with(elided.trim_start_matches(ELLIPSIS)),
                    "{elided:?} is not a tail of {path:?}"
                );
            }
        }
    }

    #[test]
    fn a_budget_too_small_for_the_ellipsis_itself_keeps_the_ellipsis() {
        assert_eq!(
            elide_path("src/main.rs", 0.5, monospace),
            Some(ELLIPSIS.to_string()),
            "nothing fits, so the row says so rather than drawing a misleading tail"
        );
        assert_eq!(
            elide_path("src/main.rs", 1., monospace),
            Some(ELLIPSIS.to_string())
        );
    }

    #[test]
    fn an_unmeasured_viewport_has_no_budget_and_a_narrow_one_has_no_negative_budget() {
        assert_eq!(header_budget(px(0.)), None);
        assert_eq!(header_budget(px(-10.)), None);
        assert_eq!(header_budget(px(1.)), Some(px(0.)));
        let wide = header_budget(px(1000.)).expect("a measured viewport has a budget");
        let narrow = header_budget(px(600.)).expect("a measured viewport has a budget");
        assert_eq!(wide - narrow, px(400.), "the furniture is a fixed width");
        assert!(narrow > px(0.));
    }

    #[test]
    fn the_path_budget_stops_a_gap_short_of_the_leftmost_bar_the_painter_can_draw() {
        let width = px(1000.);
        let budget = header_budget(width).expect("a measured viewport has a budget");
        let path_right = px(HEADER_TEXT_LEFT) + budget;

        assert_eq!(
            bar_right_edge(width, px(0.)) - px(BAR_WIDTH) - path_right,
            px(BAR_GAP),
            "the budget and the painter derive the same clearance from the same constants"
        );
        assert_eq!(
            bar_right_edge(width, px(COUNT_WIDTH + BAR_GAP)) - px(BAR_WIDTH) - path_right,
            px(0.),
            "a count wider than its column eats the gap before it reaches the path"
        );
    }

    #[test]
    fn a_bar_is_the_files_share_of_the_widest_one_in_the_patch() {
        assert_eq!(
            bar_widths(30, 10, 40),
            Some(Bar {
                added: BAR_WIDTH * 0.75,
                deleted: BAR_WIDTH * 0.25
            }),
            "the widest file fills the bar and splits it by its own two counts"
        );
        let half = bar_widths(10, 10, 40).expect("a changed file has a bar");
        assert_eq!(half.added + half.deleted, BAR_WIDTH / 2.);
        assert_eq!(half.added, half.deleted);
    }

    #[test]
    fn a_patch_that_changes_nothing_draws_no_bar_and_divides_by_nothing() {
        assert_eq!(bar_widths(0, 0, 0), None);
        assert_eq!(
            bar_widths(0, 0, 40),
            None,
            "a pure rename beside real changes is a bar of no length"
        );
    }

    #[test]
    fn a_file_of_one_kind_of_change_gets_one_segment_and_no_other() {
        assert_eq!(
            bar_widths(40, 0, 40),
            Some(Bar {
                added: BAR_WIDTH,
                deleted: 0.
            })
        );
        assert_eq!(
            bar_widths(0, 40, 40),
            Some(Bar {
                added: 0.,
                deleted: BAR_WIDTH
            })
        );
    }

    #[test]
    fn a_bar_too_short_to_draw_is_widened_rather_than_lost() {
        let one = bar_widths(1, 0, 1000).expect("one change still draws");
        assert_eq!(one.added, BAR_MIN_WIDTH);
        assert_eq!(one.deleted, 0.);

        let pair = bar_widths(1, 1, 1000).expect("two changes still draw");
        assert_eq!(pair.added, BAR_MIN_SEGMENT);
        assert_eq!(pair.deleted, BAR_MIN_SEGMENT);
    }

    #[test]
    fn a_segment_too_thin_to_see_keeps_its_minimum_without_lengthening_the_bar() {
        let lopsided = bar_widths(1, 999, 1000).expect("a changed file has a bar");
        assert_eq!(lopsided.added, BAR_MIN_SEGMENT);
        assert_eq!(lopsided.added + lopsided.deleted, BAR_WIDTH);

        let mirrored = bar_widths(999, 1, 1000).expect("a changed file has a bar");
        assert_eq!(mirrored.deleted, BAR_MIN_SEGMENT);
        assert_eq!(mirrored.added + mirrored.deleted, BAR_WIDTH);
    }

    #[test]
    fn a_pastille_tells_an_addition_a_deletion_and_a_modification_apart() {
        for theme in [ThemeColor::light(), ThemeColor::dark()] {
            let colors = [
                status_color(&FileStatus::Added, &theme),
                status_color(&FileStatus::Deleted, &theme),
                status_color(&FileStatus::Modified, &theme),
            ];
            for (i, a) in colors.iter().enumerate() {
                for (j, b) in colors.iter().enumerate().skip(i + 1) {
                    let distance = crate::theme_palette::rendered_distance(theme.secondary, *a, *b);
                    assert!(
                        distance > 0.06,
                        "statuses {i} and {j} read as the same pastille (distance {distance:.3})"
                    );
                }
            }
        }
    }

    #[test]
    fn a_move_and_a_type_change_share_one_pastille() {
        let theme = ThemeColor::light();
        let moved = status_color(&FileStatus::Renamed { similarity: 90 }, &theme);
        assert_eq!(
            status_color(&FileStatus::Copied { similarity: 90 }, &theme),
            moved
        );
        assert_eq!(status_color(&FileStatus::TypeChanged, &theme), moved);
        assert_ne!(moved, status_color(&FileStatus::Modified, &theme));
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
            "the chevron is painted beside the path, so no run and no copy can carry it"
        );
    }

    #[test]
    fn a_split_row_puts_each_side_in_its_own_column_and_pads_the_missing_one() {
        let rows = split_rows();
        assert_eq!(cell_text(&rows, 2), SharedString::from("gone"));
        assert_eq!(cell_text(&rows, 3), SharedString::default());
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
            line_background(LineOrigin::Addition, mode)
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
            bounds_for_cell(bounds, 2, px(SPLIT_CODE_LEFT), 3),
            Bounds::new(
                point(px(110. + SPLIT_CODE_LEFT), px(20. + ROW_HEIGHT)),
                size(px(100. - SPLIT_CODE_LEFT), px(ROW_HEIGHT))
            ),
            "cell 3 is the right column of the second row"
        );
        assert_eq!(
            bounds_for_cell(bounds, 1, px(CODE_LEFT), 0),
            Bounds::new(
                point(px(10. + CODE_LEFT), px(20.)),
                size(px(200. - CODE_LEFT), px(ROW_HEIGHT))
            )
        );
    }

    #[test]
    fn a_column_narrower_than_its_gutters_leaves_no_width_rather_than_a_negative_one() {
        let bounds = Bounds::new(point(px(0.), px(0.)), size(px(80.), px(400.)));
        assert_eq!(
            bounds_for_cell(bounds, 2, px(SPLIT_CODE_LEFT), 0)
                .size
                .width,
            px(0.)
        );
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
