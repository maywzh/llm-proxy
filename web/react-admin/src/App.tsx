import {
  BrowserRouter as Router,
  Routes,
  Route,
  Navigate,
} from 'react-router-dom';
import { useAuth } from './hooks/useAuth.ts';
import Layout from './components/Layout.tsx';
import Login from './pages/Login.tsx';
import Providers from './pages/Providers.tsx';
import MasterKeys from './pages/MasterKeys.tsx';

function App() {
  const { isAuthenticated } = useAuth();

  return (
    <Router>
      <Routes>
        <Route
          path="/"
          element={
            isAuthenticated ? <Navigate to="/providers" replace /> : <Login />
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
          path="/master-keys"
          element={
            isAuthenticated ? (
              <Layout>
                <MasterKeys />
              </Layout>
            ) : (
              <Navigate to="/" replace />
            )
          }
        />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Routes>
    </Router>
  );
}

export default App;
