use markup5ever_rcdom::{Node, NodeData};
use std::{borrow::Borrow, rc::Rc};

pub(crate) fn html_first<T, F>(body: &[u8], f: F) -> Option<T>
where
    F: Fn(Rc<Node>) -> Result<T, Rc<Node>>,
{
    html_cursor(body).next(f)
}

fn html_cursor(body: &[u8]) -> DomCursor {
    use html5ever::{parse_document, tendril::TendrilSink};

    let dom = parse_document(markup5ever_rcdom::RcDom::default(), Default::default())
        .from_utf8()
        .one(body);
    DomCursor::new(dom.document)
}

pub fn html_filter<T, F>(body: &[u8], f: F) -> DomIterator<F>
where
    F: Fn(Rc<Node>) -> Result<T, Rc<Node>>,
{
    html_cursor(body).into_iter(f)
}

pub struct DomCursor(Vec<Rc<Node>>);

impl DomCursor {
    pub fn new(node: Rc<Node>) -> Self {
        Self(vec![node])
    }

    pub fn next<F, T>(&mut self, f: F) -> Option<T>
    where
        F: Fn(Rc<Node>) -> Result<T, Rc<Node>>,
    {
        while let Some(node) = self.0.pop() {
            let node = match f(node) {
                Ok(e) => return Some(e),
                Err(node) => node,
            };
            self.0.extend(node.children.take().into_iter().rev());
        }
        self.0.clear();
        None
    }

    pub fn into_iter<F, T>(self, f: F) -> DomIterator<F>
    where
        F: Fn(Rc<Node>) -> Result<T, Rc<Node>>,
    {
        DomIterator::from_raw_parts(self, f)
    }
}

pub struct DomIterator<F> {
    cursor: DomCursor,
    f: F,
}

impl<F> DomIterator<F> {
    pub fn new(node: Rc<Node>, f: F) -> Self {
        Self::from_raw_parts(DomCursor::new(node), f)
    }

    pub fn from_raw_parts(cursor: DomCursor, f: F) -> Self {
        Self { cursor, f }
    }
}

impl<F: Fn(Rc<Node>) -> Result<T, Rc<Node>>, T> Iterator for DomIterator<F> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        self.cursor.next(&self.f)
    }
}

pub(crate) fn filter_tag_attr<'a>(
    tag: &'a str,
    attr: &'a str,
) -> impl Fn(Rc<Node>) -> Result<Box<str>, Rc<Node>> + 'a {
    move |node: Rc<Node>| match node.data {
        NodeData::Element {
            ref name,
            ref attrs,
            ..
        } => {
            if name.borrow().local.as_bytes() != tag.as_bytes() {
                return Err(node);
            }
            if let Some(a) = attrs.take().into_iter().find(|a| {
                a.name.local.as_bytes() == attr.as_bytes()
                    && !a.value.as_bytes().trim_ascii().is_empty()
            }) {
                Ok(a.value.to_string().into_boxed_str())
            } else {
                Err(Node::new(NodeData::Comment {
                    contents: "".into(),
                }))
            }
        }
        _ => Err(node),
    }
}
