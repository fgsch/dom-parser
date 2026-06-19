//! `html5ever` tree-sink adapter for the owned DOM representation.

use super::{Element, NodeData, NodeId, Tree};
use html5ever::interface::tree_builder::{
    ElemName, ElementFlags, NodeOrText, QuirksMode, TreeSink,
};
use html5ever::tendril::{StrTendril, TendrilSink};
use html5ever::{
    Attribute, LocalName, Namespace, ParseOpts, QualName, ns,
    parse_document as parse_html_document, parse_fragment as parse_html_fragment,
};
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashSet;

pub(super) fn parse_document(html: &str, options: ParseOpts, max_parse_errors: usize) -> Tree {
    let mut tree = parse_html_document(Sink::with_error_limit(max_parse_errors), options).one(html);
    tree.compact();
    tree
}

pub(super) fn parse_fragment(
    html: &str,
    context_element: &str,
    options: ParseOpts,
    max_parse_errors: usize,
) -> Tree {
    let context_element_allows_scripting = options.tree_builder.scripting_enabled;
    let mut tree = parse_html_fragment(
        Sink::with_error_limit(max_parse_errors),
        options,
        QualName::new(None, ns!(html), LocalName::from(context_element)),
        Vec::new(),
        context_element_allows_scripting,
    )
    .one(html);
    tree.compact();
    tree
}

#[derive(Debug)]
pub(super) struct Sink {
    pub(super) tree: RefCell<Tree>,
    max_parse_errors: usize,
}

impl Sink {
    #[cfg(test)]
    pub(super) fn new() -> Self {
        Self::with_error_limit(0)
    }

    fn with_error_limit(max_parse_errors: usize) -> Self {
        Self {
            tree: RefCell::new(Tree::new()),
            max_parse_errors,
        }
    }
}

impl TreeSink for Sink {
    type Handle = NodeId;
    type Output = Tree;
    type ElemName<'a>
        = OwnedElemName
    where
        Self: 'a;

    fn finish(self) -> Self::Output {
        self.tree.into_inner()
    }

    fn parse_error(&self, msg: Cow<'static, str>) {
        let mut tree = self.tree.borrow_mut();
        if tree.parse_errors.len() < self.max_parse_errors {
            tree.parse_errors.push(msg.into_owned());
        } else {
            tree.parse_errors_truncated = true;
        }
    }

    fn get_document(&self) -> Self::Handle {
        self.tree.borrow().document
    }

    fn elem_name<'a>(&'a self, target: &'a Self::Handle) -> Self::ElemName<'a> {
        self.tree
            .borrow()
            .element(*target)
            .expect("html5ever called elem_name on a non-element")
            .name
            .clone()
            .into()
    }

    fn create_element(
        &self,
        name: QualName,
        attrs: Vec<Attribute>,
        flags: ElementFlags,
    ) -> Self::Handle {
        let mut tree = self.tree.borrow_mut();
        let template_contents = flags.template.then(|| tree.push(NodeData::Document));

        tree.push(NodeData::Element(Element::new(
            name,
            attrs,
            template_contents,
            flags.mathml_annotation_xml_integration_point,
        )))
    }

    fn create_comment(&self, text: StrTendril) -> Self::Handle {
        self.tree.borrow_mut().push(NodeData::Comment {
            contents: text.to_string(),
        })
    }

    fn create_pi(&self, _target: StrTendril, _data: StrTendril) -> Self::Handle {
        self.tree.borrow_mut().push(NodeData::ProcessingInstruction)
    }

    fn append(&self, parent: &Self::Handle, child: NodeOrText<Self::Handle>) {
        self.tree.borrow_mut().append_or_merge_text(*parent, child);
    }

    fn append_based_on_parent_node(
        &self,
        element: &Self::Handle,
        prev_element: &Self::Handle,
        child: NodeOrText<Self::Handle>,
    ) {
        if self.tree.borrow().node(*element).parent.is_some() {
            self.append_before_sibling(element, child);
        } else {
            self.append(prev_element, child);
        }
    }

    fn append_doctype_to_document(
        &self,
        name: StrTendril,
        public_id: StrTendril,
        system_id: StrTendril,
    ) {
        let mut tree = self.tree.borrow_mut();
        let node = tree.push(NodeData::Doctype {
            name: name.to_string(),
            public_id: public_id.to_string(),
            system_id: system_id.to_string(),
        });
        let document = tree.document;
        tree.append(document, node);
    }

    fn get_template_contents(&self, target: &Self::Handle) -> Self::Handle {
        self.tree
            .borrow()
            .element(*target)
            .and_then(|element| element.template_contents)
            .expect("html5ever called get_template_contents on a non-template")
    }

    fn same_node(&self, x: &Self::Handle, y: &Self::Handle) -> bool {
        x == y
    }

    fn set_quirks_mode(&self, mode: QuirksMode) {
        self.tree.borrow_mut().quirks_mode = mode;
    }

    fn append_before_sibling(&self, sibling: &Self::Handle, new_node: NodeOrText<Self::Handle>) {
        let previous = self.tree.borrow().node(*sibling).previous_sibling;
        self.tree
            .borrow_mut()
            .insert_or_merge_text(new_node, previous, |tree, child| {
                tree.insert_before(*sibling, child);
            });
    }

    fn add_attrs_if_missing(&self, target: &Self::Handle, attrs: Vec<Attribute>) {
        let mut tree = self.tree.borrow_mut();
        let element = tree
            .element_mut(*target)
            .expect("html5ever called add_attrs_if_missing on a non-element");
        let mut existing = element
            .attrs
            .iter()
            .map(|attr| attr.name.clone())
            .collect::<HashSet<_>>();
        for attr in attrs {
            if !existing.insert(attr.name.clone()) {
                continue;
            }

            let position = element.attrs.len();
            element.add_attr_if_indexed(&attr, position);
            element.attrs.push(attr);
        }
    }

    fn remove_from_parent(&self, target: &Self::Handle) {
        self.tree.borrow_mut().detach(*target);
    }

    fn reparent_children(&self, node: &Self::Handle, new_parent: &Self::Handle) {
        let mut tree = self.tree.borrow_mut();
        let mut child = tree.node(*node).first_child;
        while let Some(id) = child {
            child = tree.node(id).next_sibling;
            tree.append(*new_parent, id);
        }
    }

    fn is_mathml_annotation_xml_integration_point(&self, target: &Self::Handle) -> bool {
        self.tree
            .borrow()
            .element(*target)
            .is_some_and(|element| element.mathml_annotation_xml_integration_point)
    }

    fn set_current_line(&self, _line_number: u64) {}
}

#[derive(Debug, Clone)]
pub(super) struct OwnedElemName(QualName);

impl From<QualName> for OwnedElemName {
    fn from(name: QualName) -> Self {
        Self(name)
    }
}

impl ElemName for OwnedElemName {
    fn ns(&self) -> &Namespace {
        &self.0.ns
    }

    fn local_name(&self) -> &LocalName {
        &self.0.local
    }
}
