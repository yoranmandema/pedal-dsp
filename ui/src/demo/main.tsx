import { render } from "preact";
import { GraphStory } from "./GraphStory";
import "../tailwind.css";

const root = document.getElementById("demo");
if (root) render(<GraphStory />, root);
;(window as any).__DEMO_RENDERED__ = true;
