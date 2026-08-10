import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./app.css";
import { Terms } from "./pages/terms";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <Terms />
  </StrictMode>,
);
