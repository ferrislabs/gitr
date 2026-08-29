use std::ops::Range;

use domain::LineOrigin;
use gpui::{
    App, Bounds, Element, ElementId, FlexDirection, GlobalElementId, HighlightStyle, Hitbox,
    HitboxBehavior, Hsla, InspectorElementId, IntoElement, LayoutId, Length, Pixels, Point,
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
    texts: Vec<StyledText>,
    selection: TextSelectionHandle,
    scroll: ScrollHandle,
    theme: ThemeColor,
    mode: ThemeMode,
    row_bounds: Vec<Bounds<Pixels>>,
    visible: Range<usize>,
}

pub(super) fn body(
    rows: Vec<Row>,
    selection: TextSelectionHandle,
    scroll: ScrollHandle,
    theme: ThemeColor,
    mode: ThemeMode,
) -> DiffBody {
    let strings: Vec<SharedString> = rows.iter().map(row_text).collect();
    let texts = rows
        .iter()
        .zip(&strings)
        .map(|(row, text)| styled_row(row, text.clone(), &theme, mode))
        .collect();
    DiffBody {
        rows,
        strings,
        texts,
        selection,
        scroll,
        theme,
        mode,
        row_bounds: Vec::new(),
        visible: 0..0,
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

    fn width(&self, text: SharedString, window: &Window) -> Pixels {
        self.shape(text, self.style.color, window).width()
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
        let viewport = self.scroll.bounds().size.height;
        if viewport <= px(0.) {
            return 0..self.rows.len();
        }
        let first = ((-self.scroll.offset().y) / px(ROW_HEIGHT)).floor().max(0.) as usize;
        let count = (viewport / px(ROW_HEIGHT)).ceil() as usize + 2;
        first.min(self.rows.len())..(first + count).min(self.rows.len())
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
        self.row_bounds = (0..self.rows.len())
            .map(|index| self.bounds_for_row(bounds, index))
            .collect();
        for (text, row_bounds) in self.texts.iter_mut().zip(&self.row_bounds) {
            text.prepaint(None, None, *row_bounds, &mut (), window, cx);
        }
        self.visible = self.visible_rows();

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
            .strings
            .iter()
            .enumerate()
            .map(|(index, text)| {
                TextSelectionRun::new(
                    text.clone(),
                    self.texts[index].layout().clone(),
                    self.row_bounds[index],
                )
                .with_document_order(index as u64)
            })
            .collect();
        let projection = self.selection.update_runs(&runs, cx);
        self.selection
            .set_fallback_copy_text(copy_text(&self.strings, projection.ranges()), cx);

        let pen = Pen::new(window);

        for index in self.visible.clone() {
            let top = self.row_bounds[index].origin.y;

            if let Some(background) = row_background(&self.rows[index], &self.theme, self.mode) {
                let band = Bounds::new(
                    point(bounds.origin.x, top),
                    size(bounds.size.width, px(ROW_HEIGHT)),
                );
                window.paint_quad(fill(band, background));
            }

            if let Some(range) = projection.ranges().get(index).and_then(Clone::clone) {
                paint_selection(
                    self.texts[index].layout(),
                    range,
                    self.theme.selection,
                    window,
                );
            }

            self.paint_gutter(index, bounds.origin.x, top, &pen, window, cx);

            let row_bounds = self.row_bounds[index];
            self.texts[index].paint(None, None, row_bounds, &mut (), &mut (), window, cx);
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
}
