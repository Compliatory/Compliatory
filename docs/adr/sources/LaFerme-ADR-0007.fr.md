# ADR-0007 — Exposer les référentiels réglementaires via MCP

- **Statut :** Accepté
- **Date :** 2026-07-24

## Contexte

L'architecture multi-agent définie dans l'ADR-0005 doit donner aux agents de cadrage,
d'évaluation, de codage, de test et de contrôle indépendant des références
réglementaires exactes. Leur transmettre une norme complète dans chaque prompt serait
à la fois coûteux en contexte, difficile à licencier et contraire à la séparation des
rôles.

Un agent doit pouvoir :

- découvrir les référentiels, éditions et profils applicables ;
- obtenir une clause ou un paragraphe précisément identifié ;
- recevoir seulement les définitions, exigences et relations nécessaires à sa phase ;
- citer le texte autorisé sans inventer une référence ;
- faire vérifier mécaniquement une citation et la couverture d'un profil.

Les textes normatifs complets sont généralement protégés et licenciés. Le produit peut
distribuer une structure et des explications originales dans les limites du droit
applicable, mais chaque entreprise doit fournir les PDF qui l'autorisent à consulter
le texte intégral. Une explication, même validée par un expert, ne devient pas pour
autant une source normative.

MCP distingue notamment les ressources, contrôlées par l'application, des outils que
le modèle peut appeler. Cette séparation convient à l'accès documentaire. En
revanche, la pagination MCP sert à parcourir des collections et ne représente pas
l'enchaînement d'une revue réglementaire.

## Décision

Un service MCP spécialisé fournira un **plan d'accès réglementaire borné, versionné
et citable**. LaFerme restera l'orchestrateur : il choisira le profil, la phase, le
rôle, l'objectif, le budget de contexte et les preuves projet accessibles. Le serveur
MCP ne décidera ni de l'applicabilité finale, ni de la conformité, ni de la prochaine
étape du workflow.

Le contrat détaillé est défini dans
[l'architecture du service MCP](../../architecture/mcp-contract.md).

### Quatre couches de contenu

Le service séparera visiblement :

1. **Catalogue** — identifiants, éditions, amendements, relations et structure dont la
   distribution est autorisée ;
2. **Guide** — explications originales approuvées par des experts, toujours marquées
   comme non normatives ;
3. **Normatif** — fragments exacts extraits d'un document licencié fourni et approuvé
   par le locataire ;
4. **Locataire** — interprétations, procédures et règles internes, séparées des
   explications livrées avec le produit.

Le serveur ne reconstruira pas un texte absent à partir d'un LLM, d'une explication
ou de la mémoire d'un modèle. Sans corpus licencié approuvé, il pourra retourner la
structure et les guides autorisés avec l'état `full_text_unavailable`, mais aucune
citation textuelle normative.

### Paquets de travail

Le serveur fabriquera des paquets de travail déterministes à partir d'entrées
explicites : profil et version, phase, rôle, objectif, références ciblées, budget de
tokens et version du corpus. Chaque paquet contiendra :

- les fragments normatifs autorisés ;
- les guides non normatifs dans un champ distinct ;
- les définitions et relations nécessaires ;
- les versions, références et empreintes des sources ;
- le budget consommé, les éléments omis et un curseur de continuation ;
- une empreinte du paquet complet.

La pagination standard de MCP restera réservée aux listes. Le curseur d'un paquet est
un concept métier : il permet à LaFerme de demander la suite d'un même objectif sans
laisser le serveur choisir une nouvelle phase.

Le budget par défaut sera de 4 096 tokens et la limite ordinaire de 8 192 tokens. Le
serveur ne coupera jamais un paragraphe normatif. Si une unité indivisible dépasse le
budget, il retournera une erreur explicite afin que l'orchestrateur augmente le budget
ou cible une unité autorisée plus précise. L'estimation emploiera le tokenizer déclaré
par le registre de modèles ; à défaut, elle utilisera un estimateur conservateur dont
l'identité apparaîtra dans le résultat.

### Référentiels pilotes

La première version du contrat couvrira trois domaines :

- IEC 62304:2006 avec l'amendement 1:2015 ;
- IEC 81001-5-1:2021 ;
- RTCA DO-178C/ED-12C.

DO-178B/ED-12B sera représentée comme une édition distincte pour les produits anciens.
Une clause, un guide ou un profil DO-178B ne pourra pas être substitué silencieusement
à son éventuel correspondant DO-178C.

### Produit hybride et multi-tenant

Le même plan de données sera déployable :

- en instance dédiée dans l'environnement d'une entreprise ;
- dans un service mutualisé isolant plusieurs locataires.

Le locataire sera toujours déduit de l'identité authentifiée et jamais choisi dans un
argument fourni par un modèle. Corpus, index, caches, clés et journaux seront isolés.
Une installation locale pourra indexer et servir ses documents sans accès à un
service externe.

### Import et autorité documentaire

L'import des PDF sera une opération administrative séparée des outils MCP accessibles
aux agents. Il comprendra au minimum :

1. déclaration de l'édition, de la langue et du droit d'utilisation ;
2. empreinte et analyse de sécurité du fichier ;
3. extraction ou OCR dans une enceinte sans réseau ;
4. rapprochement avec la structure connue et calcul d'indicateurs de confiance ;
5. comparaison et approbation par un responsable documentaire du locataire ;
6. publication d'une version immuable du corpus.

Même une extraction de confiance élevée restera en quarantaine jusqu'à l'approbation
humaine. Toute nouvelle source ou correction créera une nouvelle version et
invalidera les paquets qui dépendaient de l'ancienne.

### Limites de confiance

Les documents, guides et contenus client seront traités comme des données non fiables.
Une instruction présente dans un PDF ne deviendra jamais une commande pour le
serveur ou LaFerme. La recherche sémantique pourra aider à découvrir des passages,
mais seule une référence structurée vers un fragment approuvé pourra fonder une
citation normative.

Les prompts des rôles resteront dans LaFerme. Les primitives MCP de prompts et
l'extension MCP Tasks ne seront pas requises dans la première version. Les opérations
administratives longues pourront utiliser une file interne sans l'exposer comme un
outil invocable par les agents.

## Options examinées

### Charger les normes complètes dans chaque prompt

Rejeté. Cette approche consomme le contexte, mélange les responsabilités, favorise la
troncature et diffuse plus de texte licencié que nécessaire.

### Utiliser une base vectorielle sans contrat MCP

Rejeté comme interface principale. Un index vectoriel aide à la découverte mais ne
fournit pas à lui seul une référence stable, un contrôle de licence, un paquet
reproductible ou une validation de citation. Il pourra rester une implémentation
interne du service.

### Laisser le service documentaire piloter la revue

Rejeté. Le serveur cumulerait accès aux sources et décision sur la marche à suivre,
affaiblirait la séparation des rôles et rendrait LaFerme dépendante d'un workflow
propriétaire. LaFerme conserve l'état et le séquencement.

### Publier des résumés comme substituts aux textes licenciés

Rejeté. Un résumé peut guider la recherche mais ne garantit ni l'exhaustivité, ni les
termes exacts, ni l'applicabilité d'une exigence.

### Déployer uniquement sur site

Rejeté comme contrainte de produit. Certains clients exigeront un plan de données
local, tandis que d'autres accepteront un service hébergé. Les deux modes appliqueront
le même contrat et les mêmes barrières d'isolation.

## Conséquences

- Les agents travailleront sur des contextes plus petits et spécialisés, au prix
  d'appels documentaires supplémentaires.
- Les constats pourront référencer une version de corpus et être rejoués.
- La qualité dépendra autant de la curation humaine et du découpage que du moteur de
  recherche.
- Le produit devra maintenir un catalogue éditorial expert dans trois domaines dès la
  première version.
- Le mode SaaS imposera des tests d'isolation et une gestion des licences aussi
  importants que les fonctions de recherche.
- MCP standardisera l'accès, sans transformer le système en certificateur autonome.

## Critères de réexamen

La décision sera revue si MCP ne permet plus de préserver les URI et résultats
déterministes, si les contraintes de licence interdisent le service de fragments, si
un client impose une séparation physique incompatible avec le plan de données commun,
ou si la qualification montre qu'un budget de 8 192 tokens est insuffisant pour une
unité normative indivisible.

## Références

- [MCP — primitives serveur](https://modelcontextprotocol.io/specification/2025-06-18/server/index)
- [MCP — autorisation](https://modelcontextprotocol.io/specification/2025-06-18/basic/authorization)
- [MCP — bonnes pratiques de sécurité](https://modelcontextprotocol.io/docs/tutorials/security/security_best_practices)
- [MCP — pagination](https://modelcontextprotocol.io/specification/draft/server/utilities/pagination)
- [IEC 62304:2006+A1:2015](https://webstore.iec.ch/en/publication/6792)
- [IEC 81001-5-1:2021](https://webstore.iec.ch/en/publication/63293)
- [FAA — logiciels aéronautiques et DO-178](https://www.faa.gov/aircraft/air_cert/design_approvals/air_software/software_regs)
