// Vite client types: the project's `tsconfig.json` already lists `vite/client`
// under `types`, but Vite 7's `vite/client.d.ts` only declares CSS module and
// asset-module specifiers (`*.module.css`, `*.css?url&inline`, …). Plain
// side-effect CSS imports (the EQ panel stylesheet) need an explicit
// `declare module "*.css"` so the strict type-check (`tsc --noEmit`) passes.

/// <reference types="vite/client" />

declare module "*.css";
