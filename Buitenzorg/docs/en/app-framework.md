# Application Framework (English)

Overview

The app framework provides manifest formats, lifecycle, and windowing APIs for native and managed applications.

Manifest

- Applications include a manifest describing name, category, entrypoint, resources and permissions (filesystem access, network, audio).

Windowing & Drawing

- The kernel exposes a draw-based window API using DrawCmd structures and a present syscall. The framework builds higher-level primitives (text, layout) on top of DrawCmd.

Services

- Common platform services (package manager, theme engine, audio mixer, model manager) are available to apps through well-defined APIs and the stable syscall ABI.

Packaging

- The SDK supports templated app scaffolds, build pipelines and packaging steps. Use the bz CLI in sdk to create and package apps.