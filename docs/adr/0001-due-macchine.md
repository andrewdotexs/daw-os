# ADR 0001 — Sviluppo e destinazione su due macchine

- **Stato**: accettata
- **Data**: 13 novembre 2026
- **Origine**: D-01 del documento 0

## Contesto

Il master prompt indica Windows 11 con WSL2 per Buildroot e il 285K come hardware di riferimento, senza dire se sono la stessa macchina. La differenza decide come l'immagine arriva sul disco, come si raccolgono le misure e se una prova di 24 ore ferma lo sviluppo.

## Decisione

Due macchine. Lo sviluppo avviene su un PC Windows 11 separato con WSL2; il 285K è dedicato a dawOS e non ospita altri sistemi operativi.

## Alternative scartate

- **Una macchina sola con due dischi**: nessun hardware in più e scrittura diretta sull'NVMe da WSL2, ma ogni prova reale richiede un riavvio e le prove lunghe bloccano lo sviluppo.

## Conseguenze

- L'immagine non si può scrivere direttamente sull'NVMe di destinazione. Prima installazione da chiavetta avviabile; aggiornamenti successivi con `systemd-sysupdate` da un server HTTP sul PC di sviluppo (registro delle modifiche, sezioni P0.4 e P0.5).
- Il percorso di aggiornamento A/B si collauda fin dalla Fase 0, a ogni iterazione.
- Le due macchine devono stare sulla stessa rete cablata. La variante `dev` espone SSH con sola chiave ([ADR 0005](0005-varianti-dev-e-prod.md)).
- WSL2 si dimensiona sulle risorse del PC di sviluppo, non del 285K.
- Lo script che scrive sul disco gira sul 285K, dall'ambiente di installazione, e deve proteggere da errori di selezione del disco: identificazione per modello e numero di serie, rifiuto dei dischi di sistema, conferma digitata.
- Il carico di rete delle prove di latenza (`iperf3`) parte dal PC di sviluppo.

## Quando si riapre

Se il PC di sviluppo non regge i tempi di costruzione (prima costruzione oltre due ore), si valuta una macchina di costruzione Linux dedicata.
