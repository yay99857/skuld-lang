---
title: Concurrency
description: The current execution model and why concurrent features remain out of scope.
status: Planned
---
Skuld does not implement threads, channels, async/await or a language-level task scheduler. Networking and process APIs are blocking.

## Current runtime

Managed values use non-atomic reference counts. They are not designed to be shared across concurrently executing threads. The existence of a C foreign interface does not establish a thread-safety contract.

## Design status

Concurrency needs a language and runtime decision. This page does not prescribe future syntax, memory sharing or scheduling behavior.

## Sequential programs

Use the existing control-flow constructs and blocking APIs for current applications. Consult [memory management](/docs/language/memory-management/) before making assumptions about ownership across external boundaries.
