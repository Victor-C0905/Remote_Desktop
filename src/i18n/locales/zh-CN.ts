/** 中文词典 —— source of truth。所有其他语言包的结构必须与本文件一致（见 types.ts） */
const zhCN = {
  common: {
    confirm: '确定',
    cancel: '取消',
    retry: '重试',
  },
  connection: {
    viewServers: '查看服务器',
    reconnecting: '正在重新连接…',
    reconnectingTo: '正在重新连接到 {{name}}…',
  },
  windowShell: {
    minimize: '最小化',
    minimizeWindow: '最小化窗口',
    maximize: '最大化',
    maximizeWindow: '最大化窗口',
    close: '关闭',
    closeWindow: '关闭窗口',
  },
  errors: {
    auth: {
      missingCredentials: { title: '缺少登录信息', message: '未提供所需的登录凭据', action: '请补全服务器登录配置' },
      invalidKeyFormat: { title: '密钥格式不支持', message: '该密钥文件的格式不受支持', action: '请使用 OpenSSH 格式的密钥文件' },
      keyParseFailed: { title: '密钥无法读取', message: '密钥文件无法解析，可能已损坏或密码错误', action: '请确认密钥文件与密码' },
      certificateRejected: { title: '未信任服务器', message: '服务器证书未获信任，连接已取消', action: '如需连接请重新发起并确认证书' },
      dnsFailed: { title: '服务器地址无法解析', message: '找不到该服务器的网络地址', action: '请检查服务器地址配置' },
      connectTimeout: { title: '无法连接服务器', message: '连接超时，未能建立连接', action: '请检查网络后重试' },
      tlsHandshakeFailed: { title: '安全连接建立失败', message: '无法与服务器建立安全连接', action: '请检查网络或稍后重试' },
      networkUnreachable: { title: '网络不可达', message: '当前网络无法到达该服务器', action: '请检查网络连接' },
      streamTimeout: { title: '认证超时', message: '认证过程耗时过长', action: '请重新连接' },
      connectionLost: { title: '网络连接中断', message: '与服务器的连接已中断', action: '请检查网络后重试' },
      rateLimited: { title: '请求过于频繁', message: '短时间内连接请求过多，服务器暂时限制了访问', action: '请稍候片刻再试' },
      accountLocked: { title: '账户已临时锁定', message: '出于安全考虑，多次认证失败后账户被暂时锁定', action: '请于 15 分钟后重试' },
      invalidCredentials: { title: '用户名或密码错误', message: '服务器拒绝了当前凭据', action: '请检查后重试' },
      pubkeyNotAuthorized: { title: '密钥未获授权', message: '该密钥未被服务器授权登录', action: '请在服务器上添加公钥授权' },
      signatureVerificationFailed: { title: '密钥验证失败', message: '密钥与服务器记录不匹配', action: '请确认使用正确的密钥文件' },
      challengeExpired: { title: '认证超时', message: '认证流程耗时过长，已失效', action: '重新连接即可' },
      authServiceUnavailable: { title: '服务暂时不可用', message: '服务器暂时无法处理登录请求', action: '请稍后重试' },
      protocolError: { title: '通信异常', message: '与服务器的通信出现异常', action: '请更新客户端后重试' },
      sessionExpired: { title: '会话已超时', message: '长时间未操作，会话已结束', action: '请重新连接' },
      unknown: { title: '连接失败', message: '发生未知错误', action: '请重试' },
    },
    // 结构化错误链（Task 17 前端错误链也用）：
    terminalNotConnected: '尚未连接到服务器，请先连接后再打开终端',
    actionPrefix: '建议',
    othersConnected: 'ℹ️ 其他 {{count}} 台服务器连接正常，仅此台无法连接\n可能是本机与该服务器之间的网络问题，建议更换网络环境后重试',
  },
} as const;

export default zhCN;
