//! The command `graph`: read the description of a graph, compile it and
//! say what came out (step P1.7a). Nothing is run: it is the way to see
//! what each live thread would be given before a cycle uses it. And the
//! same reading for `run --graph`, which then runs it (step P1.7c).

use dawos_audio_io::Format;
use dawos_graph::{compile, parse, Compiled, Costs, Settings};

use crate::cli::GraphOptions;

/// The graph of `file`, compiled for `threads` live threads and
/// `format`, or what is wrong with the file. Outside the real-time
/// path: it reads a file and allocates.
pub fn load(file: &str, threads: usize, format: Format) -> Result<Compiled, String> {
    let text = std::fs::read_to_string(file).map_err(|error| format!("{file}: {error}"))?;
    let settings = Settings {
        threads,
        rate: format.rate,
        frames: format.frames,
        costs: Costs::default(),
    };
    let description = parse(&text).map_err(|error| format!("{file}: {error}"))?;
    compile(&description, &settings).map_err(|error| format!("{file}: {error}"))
}

/// The text `dawos-engine graph` prints for `options`, or what is wrong
/// with the file.
pub fn describe(options: &GraphOptions) -> Result<String, String> {
    let format = Format {
        rate: options.rate,
        frames: options
            .frames
            .unwrap_or(Format::default_frames(options.rate)),
    };
    let compiled = load(&options.file, options.threads, format)?;
    Ok(format!(
        "graph: {}\n{}",
        options.file,
        compiled.summary(options.margin_us)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(file: &str) -> GraphOptions {
        GraphOptions {
            file: file.to_string(),
            rate: 96_000,
            frames: None,
            threads: 4,
            margin_us: 150,
        }
    }

    /// Every graph that comes with the sources compiles, at every rate
    /// of the engine, on one thread and on four; one that says which
    /// thread runs a node, on four.
    #[test]
    fn the_graphs_of_the_sources_compile() {
        let directory = concat!(env!("CARGO_MANIFEST_DIR"), "/../dawos-graph/graphs");
        let mut seen = 0;
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path
                .extension()
                .is_none_or(|extension| extension != "graph")
            {
                continue;
            }
            seen += 1;
            let pinned = std::fs::read_to_string(&path).unwrap().contains("thread=");
            for rate in [44_100, 48_000, 96_000, 192_000] {
                for threads in [1, 4] {
                    if pinned && threads == 1 {
                        continue;
                    }
                    let mut options = options(path.to_str().unwrap());
                    options.rate = rate;
                    options.threads = threads;
                    let text = describe(&options).unwrap();
                    assert!(text.contains("graph_nodes:"), "{text}");
                    assert!(!text.contains("warnings:"), "{}: {text}", path.display());
                }
            }
        }
        assert!(seen >= 4, "only {seen} graphs in {directory}");
    }

    #[test]
    fn a_mistake_is_said_with_the_file_and_the_line() {
        let directory = std::env::temp_dir();
        let file = directory.join(format!("dawos-graph-test-{}.graph", std::process::id()));
        std::fs::write(&file, "noise n\ngain g\nconnect n -> g -> nowhere\n").unwrap();
        let error = describe(&options(file.to_str().unwrap())).unwrap_err();
        let _ = std::fs::remove_file(&file);
        assert!(
            error.contains("line 3: there is no node 'nowhere'"),
            "{error}"
        );
        assert!(error.contains("dawos-graph-test-"), "{error}");
        assert!(describe(&options("/nonexistent/x.graph")).is_err());
    }
}
