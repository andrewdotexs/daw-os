# ADR 0008 — `fcp-server` come processo separato

- **Stato**: accettata
- **Data**: 13 novembre 2026
- **Origine**: D-08 e M-12 del documento 0
- **Modifica una decisione vincolante**: «`dawos-fcpd` in Rust come driver FCP in userspace» del master prompt

## Contesto

Il mixer interno, il routing e le impostazioni della Scarlett 18i20 di quarta generazione si controllano con il protocollo FCP. Il kernel offre solo il trasporto (`sound/usb/fcp.c`, dispositivo hwdep, apertura con `CAP_SYS_RAWIO`); la logica sta in un programma in spazio utente. Ne esiste già uno, `fcp-server`, scritto dall'autore del driver.

## Decisione

Nell'MVP si usa **`fcp-server` così com'è**, come processo separato e servizio di sistema, unico processo con `CAP_SYS_RAWIO`. `dawos-fcpd` in Rust si scrive solo se `fcp-server` risulterà insufficiente.

Gli altri processi di dawOS parlano con la scheda attraverso i controlli ALSA che `fcp-server` pubblica, senza collegarsi al suo codice.

## Verifiche fatte

- Licenza: **GPL-3.0-or-later**, da `COPYING` e dalle intestazioni SPDX dei sorgenti _(verificato sul ramo `master` del repository `geoffreybennett/fcp-support`)_. È compatibile con la licenza di dawOS ([ADR 0010](0010-licenza-gplv3-eccezione-plugin.md)).
- Dipendenze di costruzione: alsa-lib, libcrypto di OpenSSL, zlib, json-c, libsystemd _(verificato dal `Makefile`)_. Tutte disponibili in Buildroot e con licenze compatibili.

## Alternative scartate

- **Scrivere subito `dawos-fcpd`**: mesi di lavoro su un protocollo non documentato ufficialmente, per riottenere ciò che esiste.
- **Usare il mixer della scheda solo tramite `alsamixer` o strumenti manuali**: non integrabile nel livello comandi.

## Conseguenze

- Serve un pacchetto Buildroot nostro per `fcp-server`, in `external/package/`, quando arriva l'audio (Fase 1 o Fase 5).
- Il modulo Mix cuffie userà il **monitoraggio diretto del mixer della Scarlett** tramite `fcp-server`: latenza quasi nulla per chi suona, indipendente dal quanto del motore. Ne discende che il modulo Mix cuffie dipende da `fcp-server` e dai controlli ALSA che pubblica, e che i nomi di quei controlli diventano un'interfaccia da tenere sotto versione.
- `fcp-server` va confinato come ogni altro servizio: utente dedicato, sandbox systemd, nessuna rete. Può anche aggiornare il firmware della scheda: in `prod` questa funzione va tenuta dietro un comando esplicito dell'amministratore.
- Con la licenza GPLv3 di dawOS, riusare codice di `fcp-server` in un futuro `dawos-fcpd` è lecito. Non lo è dentro un processo che carica plugin, perché a quel codice non si applica la nostra eccezione.
- I file del firmware della scheda sono di Focusrite e hanno una licenza propria: se e come distribuirli nell'immagine è da verificare.

## Quando si riapre

Se `fcp-server` non espone ciò che serve al Mix cuffie, se il suo consumo o la sua latenza disturbano i core live, o se il progetto a monte viene abbandonato.
