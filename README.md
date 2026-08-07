# restingstate: A Rust Statechart Compiler and Runtime

## 1. Project Overview & Objectives

**restingstate** is an XState-compatible statechart compiler and runtime for Rust. Rather than building a competing state-machine authoring ecosystem, the project positions itself as a **Rust execution target for XState statecharts**. 

The core philosophy is to treat the **XState machine AST** as the true source language, making the fact that it originates in a TypeScript (`.ts`) file an implementation detail. This allows developers to design and visualize state machines using the existing Stately/XState ecosystem, while deploying the execution logic to highly performant, safe, and portable Rust environments.

### Core Objectives
* **Declarative Source:** Use TypeScript/XState definitions as the portable, canonical design artifact.
* **Pure Rust Runtime:** Generate strongly typed Rust enums, structs, and `match` statements. The runtime knows nothing about TypeScript or JavaScript.
* **Portability:** Support Native, WebAssembly (WASM), and embedded execution targets.
* **Interoperability:** Full serialization/deserialization support for persistence, snapshots, and WASM messaging.
* **Compile-Time Safety:** Leverage Rust's macro system to catch missing states, invalid transitions, and unknown actions at compile time rather than runtime.

---

## 2. Architecture & Project Structure

The architecture separates the parsing, intermediate representation (IR), code generation, and runtime phases to ensure extensibility (e.g., adding JSON or SCXML frontends later).

### The Compilation Pipeline
```text
TypeScript (XState definition)
│
▼
TypeScript Parser (OXC / SWC)
│
▼
XState IR (Language-neutral Intermediate Representation)
│
Validation Passes (Graph algorithms, cycle detection)
│
▼
Rust Code Generator (quote/syn)
│
▼
Pure Rust Runtime
