import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { Browser } from "./Browser";
import "./styles.css";

if ("serviceWorker" in navigator) {
  navigator.serviceWorker.register("/sw.js").catch(() => {});
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <Browser />
  </StrictMode>
);
