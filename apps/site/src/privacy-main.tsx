import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./app.css";
import { Privacy } from "./pages/privacy";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <Privacy />
  </StrictMode>,
);
