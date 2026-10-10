# The Luma Browser Engine

Luma is an experimental browser engine written in Rust. It aims to explore how web pages are parsed, styled, laid out, and rendered by building each major stage of the rendering pipeline from scratch.

The project starts with a small subset of HTML and CSS, with the goal of gradually expanding into a more complete rendering engine over time. Rather than trying to become a production browser, Luma focuses on clarity, performance-minded design, and understanding the systems that turn source documents into pixels.

Luma is mainly a learning project for Rust, systems programming, and browser internals. It is still in the early stages and is not intended for real-world browsing.

## Features

-   HTML tokenization
-   DOM tree construction
-   CSS parsing
-   Basic selector matching
-   Style computation
-   Block layout
-   Box model calculations
-   Software-based painting
-   Local document loading

## Roadmap

> In no particular order

-   [x] HTML parser
-   [x] DOM representation
-   [x] CSS parser
-   [x] Selector matching
-   [x] Style tree
-   [x] Block layout
-   [x] Box model support
-   [ ] Text rendering
-   [ ] Painting pipeline
-   [ ] Window output
-   [x] Local file loading
-   [ ] Basic networking
-   [ ] Browser UI
-   [ ] Flexbox layout [^1]
-   [ ] JavaScript support [^2]
-   [ ] Rendering snapshots
-   [ ] Benchmarks

[^1]: Flexbox is planned after the basic block layout system is stable.
[^2]: JavaScript support is a long-term goal. The initial focus is on parsing, layout, and rendering.