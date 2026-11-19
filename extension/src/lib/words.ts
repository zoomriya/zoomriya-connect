// The extension's words, in English and French: short and natural.

export type Lang = 'en' | 'fr';

const en = {
	product: 'ZOOMRIYA',
	pairTitle: 'Pair this laptop',
	pairHelp: 'At home, on the box’s network. In Box Desktop, open Control Panel › ZOOMRIYA Connect and pick Add a laptop.',
	boxName: 'Your box’s name',
	boxNameHint: 'maplehouse.zoomriya.app',
	code: 'Code from Box Desktop',
	laptopName: 'This laptop’s name',
	laptopNameHint: 'Anna’s laptop',
	pair: 'Pair',
	pairing: 'Pairing…',
	badName: 'Type your box’s name, such as maplehouse.zoomriya.app.',
	noPermission: 'The extension needs your permission to reach your box.',
	unreachable: 'Can’t reach your box. Check this laptop is at home, on the box’s network.',
	notTheBox: 'That answer didn’t come from a ZOOMRIYA Box. Nothing was paired.',
	paired: 'Paired with',
	open: 'Open ZOOMRIYA',
	forget: 'Forget this box',
	forgetSure: 'Forget your box? You’ll have to pair this laptop again at home.',
	forgetYes: 'Forget',
	cancel: 'Cancel',
	weaker: 'The browser keeps this laptop’s key and won’t let anyone copy it. A phone’s security chip is stronger.',
	notSetUp: 'Open ZOOMRIYA once at home to finish setting up this laptop.'
};

export type Words = typeof en;

const fr: Words = {
	product: 'ZOOMRIYA',
	pairTitle: 'Associer cet ordinateur',
	pairHelp: 'À la maison, sur le réseau du boîtier. Dans Box Desktop, ouvrez Panneau de configuration › ZOOMRIYA Connect et choisissez Ajouter un ordinateur.',
	boxName: 'Le nom de votre boîtier',
	boxNameHint: 'maplehouse.zoomriya.app',
	code: 'Code de Box Desktop',
	laptopName: 'Nom de cet ordinateur',
	laptopNameHint: 'Ordinateur d’Anna',
	pair: 'Associer',
	pairing: 'Association…',
	badName: 'Saisissez le nom de votre boîtier, par exemple maplehouse.zoomriya.app.',
	noPermission: 'L’extension a besoin de votre permission pour joindre votre boîtier.',
	unreachable: 'Impossible de joindre votre boîtier. Vérifiez que cet ordinateur est à la maison, sur le réseau du boîtier.',
	notTheBox: 'Cette réponse ne vient pas d’un ZOOMRIYA Box. Rien n’a été associé.',
	paired: 'Associé à',
	open: 'Ouvrir ZOOMRIYA',
	forget: 'Oublier ce boîtier',
	forgetSure: 'Oublier votre boîtier ? Il faudra associer de nouveau cet ordinateur à la maison.',
	forgetYes: 'Oublier',
	cancel: 'Annuler',
	weaker: 'Le navigateur garde la clé de cet ordinateur et ne laisse personne la copier. La puce de sécurité d’un téléphone est plus solide.',
	notSetUp: 'Ouvrez ZOOMRIYA une fois à la maison pour finir de configurer cet ordinateur.'
};

export const words: Record<Lang, Words> = { en, fr };

/** The browser's language if we have it, otherwise English. */
export function pickLang(preferred: readonly string[]): Lang {
	for (const p of preferred) {
		const base = p.toLowerCase().split('-')[0];
		if (base === 'fr' || base === 'en') return base;
	}
	return 'en';
}
