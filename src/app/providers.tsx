"use client";

import { invoke } from "@tauri-apps/api/core";
import { useEffect } from "react";

export default function Providers({ children }: { children: React.ReactNode }) {
  return <>{children}</>;
}
