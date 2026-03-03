import { render } from "solid-js/web";
import App from "./App";
import { installRuntimeDiagnostics } from "./lib/runtime-diagnostics";
import "./styles/global.css";

installRuntimeDiagnostics();
render(() => <App />, document.getElementById("root")!);
