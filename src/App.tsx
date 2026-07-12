import { Desktop } from "./shell/Desktop";
import { StorageInitializer } from "./components/StorageInitializer";
import "./styles/variables.css"; // ✅ 新架构：只定义变量
import "./styles/base.css"; // ✅ 新架构：最小化全局样式
import "./styles/adwaita.css"; // ✅ 保留原有Adwaita样式作为备份
import "./styles/skeleton.css";

function App() {
  return (
    <StorageInitializer>
      <Desktop />
    </StorageInitializer>
  );
}

export default App;
