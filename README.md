# Orrery

> **Alpha** — under active development, not yet stable for production use.

Open source, self-hosted BPMN 2.0 workflow orchestration engine built in Rust.

[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE.md)
![Status](https://img.shields.io/badge/status-alpha-orange.svg)

---

## What is Orrery?

Orrery is a self-hosted workflow engine that runs BPMN 2.0 process definitions. It runs as a standalone HTTP server backed by PostgreSQL, exposing a REST API for deploying processes, starting instances, and managing task workers. A web UI (alpha) is bundled for visualizing and managing running process instances.

## Features

- BPMN 2.0 support: tasks, gateways, events, subprocesses
- External task workers (poll-and-complete pattern)
- PostgreSQL persistence
- REST API with interactive docs at `/docs` (Scalar UI)
- SDKs for Rust, TypeScript, and Clojure
- Web UI for process visualization (alpha)

## Quick Start

1. **Start the server** — requires PostgreSQL and a `DATABASE_URL` environment variable
2. **Deploy a BPMN process** — POST your `.bpmn` file to the REST API
3. **Start a process instance** — POST to the instances endpoint with your process key
4. **Write a worker** — use one of the SDKs to poll for tasks and complete them

→ [Full quick start guide](https://orrery-website.vercel.app/documentation/getting-started/quick-start)

## SDKs

| Language | Package | Docs |
|----------|---------|------|
| Rust | [`orrery-client`](https://github.com/orrery-io/orrery/tree/main/crates/orrery-client) | [Docs](https://orrery-website.vercel.app/documentation/sdks/rust) |
| TypeScript | [`@orrery-io/sdk`](https://github.com/orrery-io/orrery/tree/main/sdks/typescript) | [Docs](https://orrery-website.vercel.app/documentation/sdks/typescript) |
| Clojure | [`io.orrery/orrery-sdk`](https://github.com/orrery-io/orrery/tree/main/sdks/clojure) | [Docs](https://orrery-website.vercel.app/documentation/sdks/clojure) |

## Documentation

Full documentation at [orrery-website.vercel.app](https://orrery-website.vercel.app)

## License

Apache 2.0 — see [LICENSE.md](LICENSE.md)
