// The families' account page, in English and French. Short and natural.

import type { Lang } from '../lib/words';

const en = {
	product: 'ZOOMRIYA Connect',
	title: 'Your account',
	signInHelp: 'Sign in with the username and password you were given.',
	username: 'Username',
	password: 'Password',
	signIn: 'Sign in',
	signOut: 'Sign out',
	cantReach: "The server isn't answering. Try again in a minute.",
	loading: 'Loading…',
	boxes: 'Your boxes',
	noBoxes: 'No box linked yet. In Box Desktop, open Control Panel › ZOOMRIYA Connect and type this username and password.',
	boxesOf: 'Boxes linked: {n} of {max}',
	connected: 'Connected',
	notConnected: 'Not connected right now',
	devices: 'Phones and laptops connected: {n}',
	used: '{used} used through the relay this month',
	usedOf: '{used} of {allowance} used through the relay this month',
	slowed: 'Past the monthly allowance: slower through the relay until the month ends. Direct connections stay full speed.',
	changePassword: 'Change your password',
	currentPassword: 'Current password',
	newPassword: 'New password',
	repeatPassword: 'New password again',
	passwordRule: 'At least 10 characters.',
	notSame: "The new passwords don't match.",
	change: 'Change password',
	changed: 'Password changed. Sign in again with the new one.',
	theme: 'Theme',
	midnight: 'Midnight',
	daylight: 'Daylight',
	language: 'Language'
};

export type AccountWords = typeof en;

const fr: AccountWords = {
	product: 'ZOOMRIYA Connect',
	title: 'Votre compte',
	signInHelp: 'Connectez-vous avec l’identifiant et le mot de passe qu’on vous a donnés.',
	username: 'Identifiant',
	password: 'Mot de passe',
	signIn: 'Se connecter',
	signOut: 'Se déconnecter',
	cantReach: 'Le serveur ne répond pas. Réessayez dans une minute.',
	loading: 'Chargement…',
	boxes: 'Vos boîtiers',
	noBoxes: 'Aucun boîtier relié pour l’instant. Dans Box Desktop, ouvrez Panneau de configuration › ZOOMRIYA Connect et saisissez cet identifiant et ce mot de passe.',
	boxesOf: 'Boîtiers reliés : {n} sur {max}',
	connected: 'Connecté',
	notConnected: 'Pas connecté en ce moment',
	devices: 'Téléphones et ordinateurs connectés : {n}',
	used: '{used} utilisés par le relais ce mois-ci',
	usedOf: '{used} utilisés sur {allowance} par le relais ce mois-ci',
	slowed: 'Forfait du mois dépassé : plus lent par le relais jusqu’à la fin du mois. Les connexions directes restent à pleine vitesse.',
	changePassword: 'Changer de mot de passe',
	currentPassword: 'Mot de passe actuel',
	newPassword: 'Nouveau mot de passe',
	repeatPassword: 'Nouveau mot de passe, encore',
	passwordRule: 'Au moins 10 caractères.',
	notSame: 'Les nouveaux mots de passe ne correspondent pas.',
	change: 'Changer',
	changed: 'Mot de passe changé. Reconnectez-vous avec le nouveau.',
	theme: 'Thème',
	midnight: 'Minuit',
	daylight: 'Jour',
	language: 'Langue'
};

export const accountWords: Record<Lang, AccountWords> = { en, fr };
