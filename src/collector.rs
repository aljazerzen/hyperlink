use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::html::{Href, Link, UsedLink};

pub trait LinkCollector<P: Send>: Send {
    fn new() -> Self;
    fn ingest(&mut self, link: Link<'_, P>);
    fn merge(&mut self, other: Self);
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct OwnedUsedLink<P> {
    pub href: String,
    pub path: Arc<PathBuf>,
    pub paragraph: Option<P>,
}

/// Collects only used links for match-all-paragraphs command. Discards defined links.
pub struct UsedLinkCollector<P> {
    pub used_links: Vec<OwnedUsedLink<P>>,
}

impl<P: Send> LinkCollector<P> for UsedLinkCollector<P> {
    fn new() -> Self {
        UsedLinkCollector {
            used_links: Vec::new(),
        }
    }

    fn ingest(&mut self, link: Link<'_, P>) {
        if let Link::Uses(used_link) = link {
            self.used_links.push(OwnedUsedLink {
                href: used_link.href.0.to_owned(),
                path: used_link.path.to_owned(),
                paragraph: used_link.paragraph,
            });
        }
    }

    fn merge(&mut self, other: Self) {
        self.used_links.extend(other.used_links);
    }
}

#[derive(Debug)]
enum LinkState<P> {
    /// We have observed a DefinedLink for this href
    Defined,
    /// We have not *yet* observed a DefinedLink and therefore need to keep track of all link
    /// usages for potential error reporting.
    Undefined(Vec<(Arc<PathBuf>, Option<P>)>),
}

impl<P: Copy> LinkState<P> {
    fn add_usage(&mut self, link: &UsedLink<P>) {
        if let LinkState::Undefined(ref mut links) = self {
            links.push((link.path.clone(), link.paragraph));
        }
    }

    fn update(&mut self, other: Self) {
        match self {
            LinkState::Defined => (),
            LinkState::Undefined(links) => match other {
                LinkState::Defined => *self = LinkState::Defined,
                LinkState::Undefined(links2) => links.extend(links2.into_iter()),
            },
        }
    }
}

/// Link collector used for actual link checking. Keeps track of broken links only.
pub struct BrokenLinkCollector<P> {
    links: BTreeMap<String, LinkState<P>>,
    used_link_count: usize,
}

impl<P: Send + Copy> LinkCollector<P> for BrokenLinkCollector<P> {
    fn new() -> Self {
        BrokenLinkCollector {
            links: BTreeMap::new(),
            used_link_count: 0,
        }
    }

    fn ingest(&mut self, link: Link<'_, P>) {
        match link {
            Link::Uses(used_link) => {
                self.used_link_count += 1;
                self.links
                    .entry(used_link.href.0.to_owned())
                    .and_modify(|state| state.add_usage(&used_link))
                    .or_insert_with(|| {
                        let mut state = LinkState::Undefined(Vec::new());
                        state.add_usage(&used_link);
                        state
                    });
            }
            Link::Defines(defined_link) => {
                self.links
                    .insert(defined_link.href.0.to_owned(), LinkState::Defined);
            }
        }
    }

    fn merge(&mut self, other: Self) {
        self.used_link_count += other.used_link_count;

        for (href, other_state) in other.links {
            if let Some(state) = self.links.get_mut(&href) {
                state.update(other_state);
            } else {
                self.links.insert(href, other_state);
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct BrokenLink<P> {
    pub hard_404: bool,
    pub link: OwnedUsedLink<P>,
}

impl<P: Copy + PartialEq> BrokenLinkCollector<P> {
    pub fn get_broken_links(&self, check_anchors: bool) -> impl Iterator<Item = BrokenLink<P>> {
        let mut broken_links = Vec::new();

        for (href, state) in self.links.iter() {
            if let LinkState::Undefined(links) = state {
                let hard_404 = if check_anchors {
                    !matches!(
                        self.links.get(Href(href).without_anchor().0),
                        Some(&LinkState::Defined)
                    )
                } else {
                    true
                };

                for (path, paragraph) in links.iter() {
                    broken_links.push(BrokenLink {
                        hard_404,
                        link: OwnedUsedLink {
                            path: path.clone(),
                            paragraph: *paragraph,
                            href: href.clone(),
                        },
                    });
                }
            }
        }

        broken_links.into_iter()
    }

    pub fn used_links_count(&self) -> usize {
        self.used_link_count
    }
}
