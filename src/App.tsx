import { Desktop } from "./shell/Desktop";
import { StorageInitializer } from "./components/StorageInitializer";
import "./styles/variables.css";    // ✅ 层 1：全局变量定义
import "./styles/base.css";          // ✅ 层 2：最小 Reset
import "./styles/typography.css";    // ✅ 层 3：Typography 工具类
import "./styles/scrollbar.css";     // ✅ 层 4：Scrollbar 全局样式
import "./styles/skeleton.css";      // ✅ 层 5：Skeleton 全局样式

function App() {
  return (
    <StorageInitializer>
      <Desktop />
    </StorageInitializer>
  );
}

export default App;