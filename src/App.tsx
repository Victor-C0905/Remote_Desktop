import { Desktop } from "./shell/Desktop";
import { StorageInitializer } from "./components/StorageInitializer";
import "./styles/adwaita.css";
import "./styles/skeleton.css";

function App() {
  return (
    <StorageInitializer>
      <Desktop />
    </StorageInitializer>
  );
}

export default App;
