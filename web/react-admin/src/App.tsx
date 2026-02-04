import {
  BrowserRouter as Router,
  Routes,
  Route,
  Navigate,
} from 'react-router-dom';
import { useAuth } from './hooks/useAuth';
import { ToastProvider } from './contexts/ToastContext';
import { ToastContainer } from './components/Toast';
import Layout from './components/Layout.tsx';
import Login from './pages/Login.tsx';
import Providers from './pages/Providers.tsx';
import Credentials from './pages/Credentials.tsx';
import Dashboard from './pages/Dashboard.tsx';
import Health from './pages/Health.tsx';
import Chat from './pages/Chat.tsx';

function App() {
  const { isAuthenticated } = useAuth();

  return (
    <ToastProvider>
      <Router>
        <Routes>
          <Route
            path="/"
            element={
              isAuthenticated ? <Navigate to="/dashboard" replace /> : <Login />
            }
          />
          <Route
            path="/dashboard"
            element={
              isAuthenticated ? (
                <Layout>
                  <Dashboard />
                </Layout>
              ) : (
                <Navigate to="/" replace />
              )
            }
          />
          <Route
            path="/providers"
            element={
              isAuthenticated ? (
                <Layout>
                  <Providers />
                </Layout>
              ) : (
                <Navigate to="/" replace />
              )
            }
          />
          <Route
            path="/credentials"
            element={
              isAuthenticated ? (
                <Layout>
                  <Credentials />
                </Layout>
              ) : (
                <Navigate to="/" replace />
              )
            }
          />
          <Route
            path="/health"
            element={
              isAuthenticated ? (
                <Layout>
                  <Health />
                </Layout>
              ) : (
                <Navigate to="/" replace />
              )
            }
          />
          <Route
            path="/chat"
            element={
              isAuthenticated ? (
                <Layout>
                  <Chat />
                </Layout>
              ) : (
                <Navigate to="/" replace />
              )
            }
          />
          <Route path="*" element={<Navigate to="/" replace />} />
        </Routes>
      </Router>
      <ToastContainer />
    </ToastProvider>
  );
}

export default App;
