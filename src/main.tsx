import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { Browser } from "./Browser";
import "./styles.css";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <Browser />
  </StrictMode>
);
