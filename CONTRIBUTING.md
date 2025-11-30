# Contributing

Thank you for your interest in contributing to Celerrate! We welcome contributions from everyone. This document provides
guidelines and instructions for getting started.

## Code of Conduct

By participating in this project, you agree to abide by our [Code of Conduct](CODE_OF_CONDUCT.md). We are committed to
providing a welcoming and inclusive environment for all contributors.

### Prerequisites

- Rust 1.90+ ([Install Rust](https://rustup.rs/))
- Cargo (comes with Rust)
- Git
- PHP 8.0+ (for testing integrations)

### Setup

```bash
# Clone the repository
git clone https://github.com/celerrate/celerrate.git
cd celerrate

# Build the project
make build

# Run tests
make test

# Format
make format

# Lint
make lint
```

See [Makefile](Makefile) for other useful commands

## Development Workflow

### 1. Choose an Issue

- Look at [open issues](https://github.com/celerrate/celerrate/issues)
- Comment on issues you want to work on
- Ask for clarification if needed

### 2. Create a Branch

```bash
git checkout -b feature/your-feature-name
# or
git checkout -b bugfix/bug-description
```

### 3. Make Changes

Follow our code quality standards:

**Code Style**
- Format code: `cargo fmt --all`
- Check lints: `cargo clippy -- -D warnings`
- Maximum 500 lines per file
- Prefer simple, readable solutions

**Testing**
- Write tests for new functionality
- Use snapshot tests with `insta` for complex outputs
- Maintain 90%+ code coverage
- Test both happy path and error cases

**Documentation**
- Document public APIs with rustdoc
- Include code examples in documentation
- Keep comments focused on "why", not "what"

### 4. Commit Messages

Use clear, descriptive commit messages:

```
# Good
🐛 fix: handle PHP 8.1 readonly properties in AST

Readonly properties were not being correctly parsed in the AST.
This commit adds support for the readonly modifier in class properties.

Fixes #123

# Bad
fixed stuff
update
```

**Format**: `:gitmoji: <type>: <subject>`

Types:
- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation changes
- `test`: Adding or updating tests
- `refactor`: Code refactoring
- `perf`: Performance improvements
- `chore`: Maintenance tasks

A list of gitmoji is available [here](https://gitmoji.dev/)

### 5. Create a Pull Request

When opening a PR:

1. **Title**: Clear and descriptive
2. **Description**: Include:
    - What problem does this solve?
    - How does it solve it?
    - Any relevant links to issues
3. **Tests**: Include tests for new functionality
4. **Documentation**: Update docs if needed

Example PR description:

```markdown
## Description

Adds support for PHP 8.1 readonly properties in the syntax tree.

## Problem

Readonly properties introduced in PHP 8.1 were not being parsed correctly,
causing the AST to skip the readonly modifier.

## Solution

Added `readonly: bool` field to `PropertyDeclaration` AST node and updated
the tree-sitter bridge to extract the readonly modifier.

## Testing

- Added tests for readonly property parsing
- Tested with PHP 8.1 property examples
- Verified backward compatibility with PHP 8.0

Closes #123
```

## Architecture & Design Decisions

### When to Create a New Crate

A new crate is appropriate when:
- It has a single, clear responsibility
- It's used by multiple other crates
- It could be independently useful
- It keeps file sizes reasonable (< 500 LOC)

**Don't create a crate for**:
- Tiny utilities (< 100 LOC)
- Single-use modules
- Things that naturally belong together

### Adding a New Feature

Before implementing:

1. **Discuss** in an issue or discussion
2. **Design** the solution
3. **Get feedback** on the design
4. **Implement** following architecture
5. **Test thoroughly**

## Testing Guidelines

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_functionality() {
        let result = function();
        assert_eq!(result, expected);
    }

    #[test]
    #[should_panic]
    fn test_panic_on_invalid_input() {
        function("invalid");
    }
}
```

### Integration Tests

Create tests in `tests/` directory for cross-crate testing.

### Performance Tests

```rust
#[bench]
fn bench_parsing(b: &mut Bencher) {
    b.iter(|| parse(&large_file));
}
```

Run with: `cargo bench` or `make bench`

## Error Handling

Use `thiserror` for error types in libraries:

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum MyError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("parse error: {0}")]
    Parse(String),
}
```

Use `anyhow` in binaries for maximum flexibility.

## Documentation

### Rustdoc Comments

```rust
/// Short description (one line).
///
/// Longer explanation paragraph(s) if needed.
///
/// # Arguments
///
/// * `param1` - Description of param1
/// * `param2` - Description of param2
///
/// # Returns
///
/// Description of return value
///
/// # Errors
///
/// Description of error conditions
///
/// # Examples
///
/// ```
/// let result = my_function(a, b);
/// assert_eq!(result, expected);
/// ```
pub fn my_function(param1: Type1, param2: Type2) -> Result<ReturnType> {
    // Implementation
}
```
### Internal Comments

Only document "why", not "what":

```rust
// Good: Explains non-obvious logic
let n = x << 1;  // Multiply by 2 for cache alignment

// Bad: States the obvious
let n = x << 1;  // Shift left by 1
```

## Performance Considerations

### Benchmarking

Before and after performance changes:

```bash
cargo bench --all
```

## Help and Questions

- **Questions?** Start a [GitHub Discussion](https://github.com/celerrate/celerrate/discussions)
- **Bug report?** [File an issue](https://github.com/celerrate/celerrate/issues)
- **Want to chat?** Contact maintainers

## Recognition

Contributors will be recognized in:
- CHANGELOG.md (for significant contributions)
- GitHub (contributor list)
- docs/contributors.md (for major contributors)

## License

By contributing to Celerrate, you agree that your contributions will be licensed under the same licenses as the project (Apache-2.0 OR MIT).

---

Thank you for contributing to Celerrate! 🎉
