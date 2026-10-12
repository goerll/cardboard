import { createRoot } from 'react-dom/client';
import '@fontsource-variable/inter';
import './styles.css';
import { App } from './App';
import { connect } from './api';
import { applyTheme } from './settings';

applyTheme();
connect();
createRoot(document.getElementById('root')!).render(<App />);
