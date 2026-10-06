import { mount } from 'svelte';
import App from './App.svelte';
import './styles.css';
import './styles/theme.css';
// Imported after the theme files so the dark-mode flattening rules are
// injected last in the production CSS order.
import './styles/dark-mode.css';

const app = mount(App, { target: document.getElementById('app')! });

export default app;
