# Compliatory

🇬🇧 [English version](README.en.md)

Compliatory est un service MCP spécialisé qui expose des références réglementaires sous une forme
bornée, versionnée et vérifiable. Il sépare catalogue, guides non normatifs, fragments exacts issus
de corpus licenciés et règles propres au locataire.

Le serveur ne décide ni de la conformité, ni de l'applicabilité finale, ni de la prochaine étape
d'un workflow. Il fournit des ressources citables, construit des paquets de travail déterministes
et vérifie mécaniquement les citations et la couverture d'un profil.

## État du projet

Le premier vertical slice fournit :

- un serveur MCP STDIO ;
- les ressources `reg://` et cinq outils réglementaires ;
- une persistance locale SQLite et fichiers, isolée par locataire ;
- des corpus synthétiques libres de droits pour les familles pilotes ;
- un import administratif de PDF textuels avec quarantaine et approbation ;
- des empreintes, budgets et curseurs reproductibles.

Les PDF licenciés et leurs extractions ne sont jamais versionnés dans ce dépôt.

## Démarrage

```bash
cargo build --locked --workspace
cargo test --locked --workspace

# Initialise une base locale avec les fixtures synthétiques.
cargo run --locked -p compliatory-admin -- \
  --data-dir .compliatory seed-fixtures

# Lance le serveur MCP pour le compte de service local.
cargo run --locked -p compliatory-server
```

Voir [la documentation](docs/README.md) et les
[décisions d'architecture](docs/adr/README.md).
Le [guide opérateur](docs/operations.md) décrit l'identité locale et le cycle d'import approuvé.

## Limites

Compliatory ne distribue pas les textes IEC ou RTCA, ne reconstruit pas un texte manquant et ne
constitue pas un avis réglementaire ou une certification.

**Licence :** [EUPL-1.2](LICENSE.md), ou conditions commerciales séparées. Voir
[LICENSING.md](LICENSING.md).
