import { mountChat } from "./ChatImagesFixture";
import "../src/styles.css";
import "../src/glass.css";
import "../src/theme.css";
const empty = new URLSearchParams(location.search).has("empty");
mountChat(document.getElementById("root")!, empty, true);
