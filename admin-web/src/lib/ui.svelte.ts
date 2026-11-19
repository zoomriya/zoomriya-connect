// The GUI's own state: language, theme, who's signed in.

import { pickLang, words, type Lang } from './words';

function load(key: string): string | null {
	try {
		return localStorage.getItem(key);
	} catch {
		return null;
	}
}

function save(key: string, value: string): void {
	try {
		localStorage.setItem(key, value);
	} catch {
		/* not kept: fine */
	}
}

const storedLang = load('zoomriya-connect-admin-lang');
const storedTheme = load('zoomriya-connect-admin-theme');

class Ui {
	lang = $state<Lang>(
		storedLang === 'en' || storedLang === 'fr' ? storedLang : pickLang(navigator.languages ?? [])
	);
	theme = $state<'midnight' | 'daylight'>(storedTheme === 'daylight' ? 'daylight' : 'midnight');
	w = $derived(words[this.lang]);

	setLang(l: Lang) {
		this.lang = l;
		save('zoomriya-connect-admin-lang', l);
		document.documentElement.lang = l;
	}

	setTheme(t: 'midnight' | 'daylight') {
		this.theme = t;
		save('zoomriya-connect-admin-theme', t);
		document.documentElement.classList.toggle('midnight', t === 'midnight');
	}
}

export const ui = new Ui();
document.documentElement.classList.toggle('midnight', ui.theme === 'midnight');
document.documentElement.lang = ui.lang;
