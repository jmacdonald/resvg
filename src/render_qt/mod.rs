// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Qt backend implementation.

// external
use qt;

// self
use tree::prelude::*;
use math::*;
use traits::{
    ConvTransform,
    TransformFromBBox,
};
use {
    ErrorKind,
    Options,
    OutputImage,
    Render,
    Result,
};
use utils;


macro_rules! try_create_image {
    ($size:expr, $ret:expr) => {
        try_opt_warn!(
            qt::Image::new($size.width as u32, $size.height as u32),
            $ret,
            "Failed to create a {}x{} image.", $size.width, $size.height
        );
    };
}


mod clippath;
mod fill;
mod gradient;
mod image;
mod path;
mod pattern;
mod stroke;
mod text;


impl ConvTransform<qt::Transform> for tree::Transform {
    fn to_native(&self) -> qt::Transform {
        qt::Transform::new(self.a, self.b, self.c, self.d, self.e, self.f)
    }

    fn from_native(ts: &qt::Transform) -> Self {
        let d = ts.data();
        Self::new(d.0, d.1, d.2, d.3, d.4, d.5)
    }
}

impl TransformFromBBox for qt::Transform {
    fn from_bbox(bbox: Rect) -> Self {
        debug_assert!(!bbox.width().is_fuzzy_zero());
        debug_assert!(!bbox.height().is_fuzzy_zero());

        Self::new(bbox.width(), 0.0, 0.0, bbox.height(), bbox.x(), bbox.y())
    }
}

/// Cairo backend handle.
#[derive(Clone, Copy)]
pub struct Backend;

impl Render for Backend {
    fn render_to_image(
        &self,
        rtree: &tree::RenderTree,
        opt: &Options,
    ) -> Result<Box<OutputImage>> {
        let img = render_to_image(rtree, opt)?;
        Ok(Box::new(img))
    }

    fn render_node_to_image(
        &self,
        node: tree::NodeRef,
        opt: &Options,
    ) -> Result<Box<OutputImage>> {
        let img = render_node_to_image(node, opt)?;
        Ok(Box::new(img))
    }

    fn calc_node_bbox(
        &self,
        node: tree::NodeRef,
        opt: &Options,
    ) -> Option<Rect> {
        calc_node_bbox(node, opt)
    }
}

impl OutputImage for qt::Image {
    fn save(&self, path: &::std::path::Path) -> bool {
        self.save(path.to_str().unwrap())
    }
}


/// Renders SVG to image.
pub fn render_to_image(
    rtree: &tree::RenderTree,
    opt: &Options,
) -> Result<qt::Image> {
    let (img, img_size) = create_image(rtree.svg_node().size.to_screen_size(), opt)?;

    let painter = qt::Painter::new(&img);
    render_to_canvas(rtree, opt, img_size, &painter);
    painter.end();

    Ok(img)
}

/// Renders SVG node to image.
pub fn render_node_to_image(
    node: tree::NodeRef,
    opt: &Options,
) -> Result<qt::Image> {
    let node_bbox = if let Some(bbox) = calc_node_bbox(node, opt) {
        bbox
    } else {
        // TODO: custom error
        warn!("Node {:?} has zero size.", node.svg_id());
        return Err(ErrorKind::NoCanvas.into());
    };

    let vbox = tree::ViewBox {
        rect: node_bbox,
        .. tree::ViewBox::default()
    };

    let (img, img_size) = create_image(node_bbox.size.to_screen_size(), opt)?;

    let painter = qt::Painter::new(&img);
    render_node_to_canvas(node, opt, vbox, img_size, &painter);
    painter.end();

    Ok(img)
}

fn create_image(
    size: ScreenSize,
    opt: &Options,
) -> Result<(qt::Image, ScreenSize)> {
    let img_size = utils::fit_to(size, opt.fit_to);

    debug_assert!(!img_size.is_empty_or_negative());

    let mut img = try_create_image!(img_size, Err(ErrorKind::NoCanvas.into()));

    // Fill background.
    if let Some(c) = opt.background {
        img.fill(c.red, c.green, c.blue, 255);
    } else {
        img.fill(0, 0, 0, 0);
    }
    img.set_dpi(opt.dpi);

    Ok((img, img_size))
}

/// Renders SVG to canvas.
pub fn render_to_canvas(
    rtree: &tree::RenderTree,
    opt: &Options,
    img_size: ScreenSize,
    painter: &qt::Painter,
) {
    apply_viewbox_transform(rtree.svg_node().view_box, img_size, painter);
    render_group(rtree.root(), opt, img_size, &painter);
}

/// Renders SVG node to canvas.
pub fn render_node_to_canvas(
    node: tree::NodeRef,
    opt: &Options,
    view_box: tree::ViewBox,
    img_size: ScreenSize,
    painter: &qt::Painter,
) {
    apply_viewbox_transform(view_box, img_size, &painter);

    let curr_ts = painter.get_transform();
    let mut ts = utils::abs_transform(node);
    ts.append(&node.transform());

    painter.apply_transform(&ts.to_native());
    render_node(node, opt, img_size, painter);
    painter.set_transform(&curr_ts);
}

/// Applies viewbox transformation to the painter.
pub fn apply_viewbox_transform(
    view_box: tree::ViewBox,
    img_size: ScreenSize,
    painter: &qt::Painter,
) {
    let ts = {
        let (dx, dy, sx, sy) = utils::view_box_transform(view_box, img_size);
        qt::Transform::new(sx, 0.0, 0.0, sy, dx, dy)
    };
    painter.apply_transform(&ts);
}

// TODO: render groups backward to reduce memory usage
//       current implementation keeps parent canvas until all children are rendered
fn render_group(
    parent: tree::NodeRef,
    opt: &Options,
    img_size: ScreenSize,
    p: &qt::Painter,
) -> Rect {
    let curr_ts = p.get_transform();
    let mut g_bbox = Rect::new_bbox();

    for node in parent.children() {
        p.apply_transform(&node.transform().to_native());

        let bbox = render_node(node, opt, img_size, p);

        if let Some(bbox) = bbox {
            g_bbox.expand(bbox);
        }

        // Revert transform.
        p.set_transform(&curr_ts);
    }

    g_bbox
}

fn render_group_impl(
    node: tree::NodeRef,
    g: &tree::Group,
    opt: &Options,
    img_size: ScreenSize,
    p: &qt::Painter,
) -> Option<Rect> {
    let mut sub_img = try_create_image!(img_size, None);

    sub_img.fill(0, 0, 0, 0);
    sub_img.set_dpi(opt.dpi);

    let sub_p = qt::Painter::new(&sub_img);
    sub_p.set_transform(&p.get_transform());

    let bbox = render_group(node, opt, img_size, &sub_p);

    if let Some(idx) = g.clip_path {
        if let Some(clip_node) = node.tree().defs_at(idx) {
            if let tree::NodeKind::ClipPath(ref cp) = *clip_node.value() {
                clippath::apply(clip_node, cp, opt, bbox, img_size, &sub_p);
            }
        }
    }

    sub_p.end();

    if let Some(opacity) = g.opacity {
        p.set_opacity(opacity);
    }

    let curr_ts = p.get_transform();
    p.set_transform(&qt::Transform::default());

    p.draw_image(0.0, 0.0, &sub_img);

    p.set_opacity(1.0);
    p.set_transform(&curr_ts);

    Some(bbox)
}

fn render_node(
    node: tree::NodeRef,
    opt: &Options,
    img_size: ScreenSize,
    p: &qt::Painter,
) -> Option<Rect> {
    match *node.value() {
        tree::NodeKind::Path(ref path) => {
            Some(path::draw(node.tree(), path, opt, p))
        }
        tree::NodeKind::Text(_) => {
            Some(text::draw(node, opt, p))
        }
        tree::NodeKind::Image(ref img) => {
            Some(image::draw(img, p))
        }
        tree::NodeKind::Group(ref g) => {
            render_group_impl(node, g, opt, img_size, p)
        }
        _ => None,
    }
}

/// Calculates node's absolute bounding box.
///
/// Note: this method can be pretty expensive.
pub fn calc_node_bbox(
    node: tree::NodeRef,
    opt: &Options,
) -> Option<Rect> {
    // Unwrap can't fail, because `None` will be returned only on OOM,
    // and we cannot hit it with a such small image.
    let mut img = qt::Image::new(1, 1).unwrap();
    img.set_dpi(opt.dpi);
    let p = qt::Painter::new(&img);

    let abs_ts = utils::abs_transform(node);
    _calc_node_bbox(node, abs_ts, &p)
}

fn _calc_node_bbox(
    node: tree::NodeRef,
    ts: tree::Transform,
    p: &qt::Painter,
) -> Option<Rect> {
    let mut ts2 = ts;
    ts2.append(&node.transform());

    match *node.value() {
        tree::NodeKind::Path(ref path) => {
            Some(utils::path_bbox(&path.segments, path.stroke.as_ref(), &ts2))
        }
        tree::NodeKind::Text(_) => {
            let mut bbox = Rect::new_bbox();

            text::draw_tspan(node, p, |tspan, x, y, _, font| {
                let mut p_path = qt::PainterPath::new();
                p_path.add_text(x, y, font, &tspan.text);

                let segments = from_qt_path(&p_path);

                if !segments.is_empty() {
                    let c_bbox = utils::path_bbox(&segments, tspan.stroke.as_ref(), &ts2);

                    bbox.expand(c_bbox);
                }
            });

            Some(bbox)
        }
        tree::NodeKind::Image(ref img) => {
            let segments = utils::rect_to_path(img.view_box.rect);
            Some(utils::path_bbox(&segments, None, &ts2))
        }
        tree::NodeKind::Group(_) => {
            let mut bbox = Rect::new_bbox();

            for child in node.children() {
                if let Some(c_bbox) = _calc_node_bbox(child, ts2, p) {
                    bbox.expand(c_bbox);
                }
            }

            Some(bbox)
        }
        _ => None
    }
}

fn from_qt_path(p_path: &qt::PainterPath) -> Vec<tree::PathSegment> {
    let mut segments = Vec::with_capacity(p_path.len() as usize);
    let p_path_len = p_path.len();
    let mut i = 0;
    while i < p_path_len {
        let (kind, x, y) = p_path.get(i);
        match kind {
            qt::PathSegmentType::MoveToSegment => {
                segments.push(tree::PathSegment::MoveTo { x, y });
            }
            qt::PathSegmentType::LineToSegment => {
                segments.push(tree::PathSegment::LineTo { x, y });
            }
            qt::PathSegmentType::CurveToSegment => {
                let (_, x1, y1) = p_path.get(i + 1);
                let (_, x2, y2) = p_path.get(i + 2);

                segments.push(tree::PathSegment::CurveTo { x1, y1, x2, y2, x, y });

                i += 2;
            }
        }

        i += 1;
    }

    if segments.len() < 2 {
        segments.clear();
    }

    segments
}
