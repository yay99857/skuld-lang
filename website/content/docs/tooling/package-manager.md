---
title: Package manager
description: Current module distribution and the status of package tooling.
status: Planned
---
Skuld has no package manager, package registry or project manifest today. There is no supported package-install command.

## What to use now

Keep local modules inside the program root and import their directory paths. The standard library is embedded in the compiler and needs no separate installation.

Native dependencies are linked explicitly with `-l` and `-L`; this does not download or manage those libraries.

## Future work

Package naming, dependency resolution, manifests and registry behavior are not specified here. They require their own design decisions. See [modules](/docs/language/modules/) for the implemented source organization model.
