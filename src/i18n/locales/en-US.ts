import type { TranslationSchema } from '../types';

/** English 词典。类型约束：缺 key / 多 key / 结构不一致 → 编译报错 */
const enUS: TranslationSchema = {
  common: {
    confirm: 'OK',
    cancel: 'Cancel',
  },
  errors: {
    auth: {
      missingCredentials: { title: 'Missing sign-in info', message: 'Required sign-in credentials were not provided', action: 'Complete the server sign-in configuration' },
      invalidKeyFormat: { title: 'Unsupported key format', message: 'This key file format is not supported', action: 'Use an OpenSSH-format key file' },
      keyParseFailed: { title: 'Cannot read key', message: 'The key file could not be parsed; it may be corrupted or the passphrase is wrong', action: 'Check the key file and passphrase' },
      certificateRejected: { title: 'Untrusted server', message: 'The server certificate is not trusted; the connection was cancelled', action: 'Reconnect and accept the certificate if you want to proceed' },
      dnsFailed: { title: 'Cannot resolve server address', message: 'The network address of this server could not be found', action: 'Check the server address settings' },
      connectTimeout: { title: 'Cannot connect to server', message: 'The connection timed out', action: 'Check your network and try again' },
      tlsHandshakeFailed: { title: 'Secure connection failed', message: 'Could not establish a secure connection to the server', action: 'Check your network or try again later' },
      networkUnreachable: { title: 'Network unreachable', message: 'The current network cannot reach this server', action: 'Check your network connection' },
      streamTimeout: { title: 'Authentication timed out', message: 'Authentication took too long', action: 'Reconnect' },
      connectionLost: { title: 'Network connection lost', message: 'The connection to the server was lost', action: 'Check your network and try again' },
      rateLimited: { title: 'Too many requests', message: 'Too many connection attempts in a short time; the server has temporarily limited access', action: 'Wait a moment and try again' },
      accountLocked: { title: 'Account temporarily locked', message: 'For security reasons the account was locked after repeated authentication failures', action: 'Try again in 15 minutes' },
      invalidCredentials: { title: 'Wrong username or password', message: 'The server rejected the current credentials', action: 'Check them and try again' },
      pubkeyNotAuthorized: { title: 'Key not authorized', message: 'This key is not authorized to sign in on the server', action: "Add the public key's authorization on the server" },
      signatureVerificationFailed: { title: 'Key verification failed', message: "The key does not match the server's records", action: 'Make sure you are using the correct key file' },
      challengeExpired: { title: 'Authentication timed out', message: 'The authentication flow took too long and expired', action: 'Just reconnect' },
      authServiceUnavailable: { title: 'Service temporarily unavailable', message: 'The server cannot process sign-in requests right now', action: 'Try again later' },
      protocolError: { title: 'Communication error', message: 'Communication with the server behaved unexpectedly', action: 'Update the client and try again' },
      sessionExpired: { title: 'Session expired', message: 'The session ended after a long period of inactivity', action: 'Reconnect' },
      unknown: { title: 'Connection failed', message: 'An unknown error occurred', action: 'Try again' },
    },
    terminalNotConnected: 'Not connected to a server yet. Connect before opening a terminal',
    actionPrefix: 'Tip',
    othersConnected: 'ℹ️ {{count}} other server(s) are connected fine; only this one fails\nThis is likely a network issue between this machine and the server. Try a different network and retry',
  },
};

export default enUS;
