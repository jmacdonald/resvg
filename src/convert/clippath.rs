// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

// external
use svgdom;

// self
use tree;
use short::{
    AId,
    EId,
};
use traits::{
    GetValue,
};
use super::{
    path,
    text,
    shapes,
};


pub fn convert(
    node: &svgdom::Node,
    rtree: &mut tree::RenderTree,
) {
    let attrs = node.attributes();

    let clip_node = rtree.append_defs(
        tree::NodeKind::ClipPath(tree::ClipPath {
            id: node.id().clone(),
            units: super::convert_element_units(&attrs, AId::ClipPathUnits),
            transform: attrs.get_transform(AId::Transform).unwrap_or_default(),
        })
    );

    convert_children(node, clip_node, rtree);

    // TODO: this
//    if rtree.node_at(idx).children().count() == 0 {
//        warn!("The '{}' clipPath has no valid children. Skipped.", node.id());
//        rtree.remove_node(idx);
//    }
}

fn convert_children(
    node: &svgdom::Node,
    parent: tree::NodeId,
    rtree: &mut tree::RenderTree,
) {
    for (id, node) in node.children().svg() {
        match id {
              EId::Line
            | EId::Rect
            | EId::Polyline
            | EId::Polygon
            | EId::Circle
            | EId::Ellipse => {
                if let Some(d) = shapes::convert(&node) {
                    path::convert(&node, d, parent, rtree);
                }
            }
            EId::Path => {
                let attrs = node.attributes();
                if let Some(d) = attrs.get_path(AId::D) {
                    path::convert(&node, d.clone(), parent, rtree);
                }
            }
            EId::Text => {
                text::convert(&node, parent, rtree);
            }
            _ => {
                warn!("Skipping the '{}' clipPath invalid child element '{}'.",
                      node.id(), id);
                continue;
            }
        }
    }
}