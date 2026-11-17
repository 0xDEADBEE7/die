# fastgrep

A small, fast line-oriented search tool. It walks your tree in parallel, respects `.gitignore`, and prints matches as it finds them.

[Installation](#installation) · [Usage](#usage) · [Configuration](#configuration) · [License](#license)

## Why another one?

Most of the time you want three things from a search tool: that it is **fast**, that it skips what you never want to see, and that its output can be read by people *and* programs. fastgrep does those and little else.

> [!NOTE]
> fastgrep is not a drop-in replacement for `grep`. Its flags are its own.

## Installation

With cargo:

```sh
cargo install fastgrep
```

Or from a release:

| Platform | Architecture | Download |
| -------- | ------------ | -------- |
| macOS | arm64 | `fastgrep-aarch64-apple-darwin.tar.gz` |
| macOS | x86_64 | `fastgrep-x86_64-apple-darwin.tar.gz` |
| Linux | x86_64 | `fastgrep-x86_64-unknown-linux-musl.tar.gz` |

## Usage

Search the current directory for a pattern:

```sh
fg 'fn \w+_test' --type rust
```

Common flags:

- `-i`, `--ignore-case`: case-insensitive search
- `-w`, `--word`: match whole words only
- `-t`, `--type <TYPE>`: only search files of this type
  - `rust`, `py`, `js`, `md`, and so on
  - run `fg --type-list` for all of them
- `-C <N>`: show `N` lines of context

### From a program

```rust
use fastgrep::{Searcher, Sink};

fn main() -> Result<(), fastgrep::Error> {
    let searcher = Searcher::new("TODO")?.ignore_case(true);
    for hit in searcher.walk(".")? {
        println!("{}:{}: {}", hit.path.display(), hit.line, hit.text);
    }
    Ok(())
}
```

## Configuration

fastgrep reads `~/.config/fastgrep/config.toml`:

```toml
[defaults]
ignore_case = true
context = 2

[types]
web = ["*.html", "*.css", "*.js"]
```

1. Command-line flags win over the file.
2. The file wins over the built-in defaults.
3. `FASTGREP_CONFIG` names another file.

## Roadmap

- [x] Parallel walk
- [x] `.gitignore` support
- [ ] Compressed files
- [ ] Multi-line patterns

## License

MIT or Apache-2.0, at your option. See [LICENSE-MIT](LICENSE-MIT) and [LICENSE-APACHE](LICENSE-APACHE).
