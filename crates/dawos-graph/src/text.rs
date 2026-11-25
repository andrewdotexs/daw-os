//! The description of a graph, as text.
//!
//! One line per node, the kind first, then the name, then the
//! parameters:
//!
//! ```text
//! # A tone that comes back through the cable is made softer and sent
//! # to output 5.
//! input   in3   channel=3
//! gain    soft  db=-6
//! output  out5  channel=5
//! connect in3 -> soft -> out5
//! ```
//!
//! and one line per chain of connections, from left to right. A node
//! with more than one port names it after a colon, from 0: `left ->
//! comp:0`, `right -> comp:1`; in the middle of a chain the port is both
//! the one the signal enters by and the one it leaves by. A `sum` and a
//! `bus` take as many inputs as are connected, in no order.
//!
//! `thread=N`, on any node, keeps it on live thread N whatever the
//! compiler would choose. `#` starts a comment. The order of the lines
//! does not matter.
//!
//! This is a format for tests and measures, written by hand and read by
//! a person: from Phase 2 the graph comes from the control plane.

use crate::kind::Kind;
use crate::Error;

/// A node as declared.
#[derive(Debug, Clone, PartialEq)]
pub struct Declared {
    /// Its name, unique in the description.
    pub name: String,
    /// What it does.
    pub kind: Kind,
    /// The live thread it must run on, when the description says so.
    pub thread: Option<usize>,
    /// The line that declares it, from 1.
    pub line: usize,
}

/// One end of a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct End {
    /// The node, as its place in [`Description::nodes`].
    pub node: usize,
    /// The port, when the description names one.
    pub port: Option<u32>,
}

/// A connection from an output port to an input port.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Link {
    /// Where the signal comes from.
    pub from: End,
    /// Where it goes.
    pub to: End,
    /// The line that makes it, from 1.
    pub line: usize,
}

/// A graph as described: nodes and connections, nothing checked beyond
/// the words.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Description {
    /// The nodes, in the order of the text.
    pub nodes: Vec<Declared>,
    /// The connections, in the order of the text.
    pub links: Vec<Link>,
}

/// A name and maybe a port, as written in a chain.
struct Written<'a> {
    name: &'a str,
    port: Option<u32>,
}

/// Whether `name` can be the name of a node: a letter or an underscore,
/// then letters, digits, underscores, dots and hyphens.
fn is_name(name: &str) -> bool {
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && characters.all(|other| other.is_ascii_alphanumeric() || matches!(other, '_' | '.' | '-'))
}

/// One element of a chain: `name` or `name:port`.
fn written(element: &str, line: usize) -> Result<Written<'_>, Error> {
    let (name, port) = match element.split_once(':') {
        None => (element, None),
        Some((name, port)) => {
            let port = port.parse::<u32>().map_err(|_| {
                Error::new(
                    line,
                    format!("'{element}': after the colon comes the number of a port, from 0"),
                )
            })?;
            (name, Some(port))
        }
    };
    if !is_name(name) {
        return Err(Error::new(
            line,
            format!("'{name}' is not the name of a node"),
        ));
    }
    Ok(Written { name, port })
}

/// Read a description.
pub fn parse(text: &str) -> Result<Description, Error> {
    let mut description = Description::default();
    // Chains are resolved at the end: a node may be declared after the
    // line that connects it.
    let mut chains: Vec<(usize, Vec<Written<'_>>)> = Vec::new();

    for (index, whole) in text.lines().enumerate() {
        let line = index + 1;
        let content = whole.split('#').next().unwrap_or("").trim();
        if content.is_empty() {
            continue;
        }
        let (word, rest) = content
            .split_once(char::is_whitespace)
            .map_or((content, ""), |(word, rest)| (word, rest.trim()));

        if word == "connect" {
            let elements: Vec<&str> = rest.split("->").map(str::trim).collect();
            if elements.len() < 2 || elements.iter().any(|element| element.is_empty()) {
                return Err(Error::new(
                    line,
                    "a connection is written 'connect A -> B', with as many '-> C' as wanted",
                ));
            }
            let chain = elements
                .into_iter()
                .map(|element| written(element, line))
                .collect::<Result<Vec<_>, _>>()?;
            chains.push((line, chain));
            continue;
        }

        let mut words = rest.split_whitespace();
        let name = words
            .next()
            .ok_or_else(|| Error::new(line, format!("'{word}' needs a name: '{word} NAME ...'")))?;
        if !is_name(name) {
            return Err(Error::new(
                line,
                format!(
                    "'{name}' cannot be a name: a letter, then letters, digits, '_', '.' or '-'"
                ),
            ));
        }
        if let Some(other) = description.nodes.iter().find(|node| node.name == name) {
            return Err(Error::new(
                line,
                format!("'{name}' is already declared on line {}", other.line),
            ));
        }
        let mut parameters = Vec::new();
        let mut thread = None;
        for parameter in words {
            let (key, value) = parameter.split_once('=').ok_or_else(|| {
                Error::new(
                    line,
                    format!("'{parameter}': a parameter is written name=value"),
                )
            })?;
            if key == "thread" {
                thread = Some(value.parse::<usize>().map_err(|_| {
                    Error::new(
                        line,
                        format!("thread={value}: the number of a live thread, from 0"),
                    )
                })?);
            } else if parameters.iter().any(|(other, _)| *other == key) {
                return Err(Error::new(line, format!("'{key}' is given twice")));
            } else {
                parameters.push((key, value));
            }
        }
        description.nodes.push(Declared {
            name: name.to_string(),
            kind: Kind::from_words(word, &parameters, line)?,
            thread,
            line,
        });
    }

    for (line, chain) in chains {
        let ends = chain
            .iter()
            .map(|written| {
                description
                    .nodes
                    .iter()
                    .position(|node| node.name == written.name)
                    .map(|node| End {
                        node,
                        port: written.port,
                    })
                    .ok_or_else(|| Error::new(line, format!("there is no node '{}'", written.name)))
            })
            .collect::<Result<Vec<_>, _>>()?;
        for pair in ends.windows(2) {
            description.links.push(Link {
                from: pair[0],
                to: pair[1],
                line,
            });
        }
    }
    Ok(description)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nodes_and_a_chain() {
        let description = parse(
            "# softer\n\
             input   in3   channel=3\n\
             gain    soft  db=-6   # half\n\
             \n\
             output  out5  channel=5\n\
             connect in3 -> soft -> out5\n",
        )
        .unwrap();
        assert_eq!(description.nodes.len(), 3);
        assert_eq!(description.nodes[1].name, "soft");
        assert_eq!(description.nodes[1].kind, Kind::Gain { db: -6.0 });
        assert_eq!(description.nodes[1].line, 3);
        assert_eq!(description.links.len(), 2);
        assert_eq!(description.links[0].from.node, 0);
        assert_eq!(description.links[0].to.node, 1);
        assert_eq!(description.links[1].from.node, 1);
        assert_eq!(description.links[1].to.node, 2);
        assert_eq!(description.links[1].line, 6);
    }

    #[test]
    fn ports_a_pinned_thread_and_a_node_declared_later() {
        let description = parse(
            "connect a -> l:1 -> m\n\
             noise a\n\
             load l class=heavy thread=2\n\
             sum m\n\
             connect a->l:0\n",
        )
        .unwrap();
        assert_eq!(description.nodes[1].thread, Some(2));
        assert_eq!(description.nodes[0].thread, None);
        // In the middle of a chain the port is the one in and the one
        // out.
        assert_eq!(description.links[0].to.port, Some(1));
        assert_eq!(description.links[1].from.port, Some(1));
        assert_eq!(description.links[1].to.port, None);
        assert_eq!(description.links[2].to.port, Some(0));
    }

    #[test]
    fn what_is_wrong_is_said_with_the_line() {
        let cases = [
            ("gain\n", 1, "needs a name"),
            ("gain 3db\n", 1, "cannot be a name"),
            ("sum a\nsum a\n", 2, "already declared on line 1"),
            ("gain a db\n", 1, "name=value"),
            ("gain a db=1 db=2\n", 1, "given twice"),
            ("gain a thread=x\n", 1, "the number of a live thread"),
            ("sum a\nconnect a\n", 2, "connect A -> B"),
            ("sum a\nconnect a -> \n", 2, "connect A -> B"),
            ("sum a\nconnect a -> b\n", 2, "there is no node 'b'"),
            (
                "sum a\nsum b\nconnect a:x -> b\n",
                3,
                "the number of a port",
            ),
            ("\n\nwhat a\n", 3, "no kind of node 'what'"),
        ];
        for (text, line, expected) in cases {
            let error = parse(text).unwrap_err();
            assert_eq!(error.line, line, "{text}");
            assert!(error.message.contains(expected), "{}", error.message);
        }
    }

    #[test]
    fn nothing_but_comments_is_an_empty_description() {
        assert_eq!(parse("# nothing\n\n").unwrap(), Description::default());
    }
}
