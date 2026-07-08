/// <reference types="vite/client" />

// TypeScript 6 checks side-effect imports (TS2882); this package ships only CSS.
declare module "@fontsource-variable/inter";

// Compile-time constant from the `define` in vite.config.ts.
declare const __APP_VERSION__: string;
