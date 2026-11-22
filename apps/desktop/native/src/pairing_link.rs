use crate::{Desktop, Request, workspace::action};
use rune::{El, row, spacer, tok};
use rune_kit::Dialog;
use serde::Deserialize;

#[derive(Deserialize, PartialEq)]
pub(super) struct Offer {
    origin: String,
    link: String,
}

impl Desktop {
    fn accept_pairing_offer(&mut self) {
        if self.pending {
            return;
        }
        if let Some(offer) = self.pairing_offers.pop_front() {
            self.send(Request::Pair { link: &offer.link });
        }
    }
}

pub(super) fn overlay(s: &Desktop, theme: &tok::Theme, dark: bool, overlays: &mut Vec<El>) {
    let Some(offer) = s.pairing_offers.front() else {
        return;
    };
    let cancel = |s: &mut Desktop| {
        s.pairing_offers.pop_front();
    };
    Dialog::new("pairing-offer", format!("Connect to {}?", offer.origin))
        .description("This will open the environment in this window.")
        .open(true)
        .fit()
        .width(432.)
        .child(
            row()
                .w_full()
                .gap(8.)
                .child(spacer())
                .child(action("Cancel", theme, true, cancel))
                .child(action(
                    "Connect",
                    theme,
                    !s.pending,
                    Desktop::accept_pairing_offer,
                )),
        )
        .on_close(cancel)
        .build(&s.contrast.kit(dark), overlays);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Reply, WorkerMessage};

    #[test]
    fn pending_offers_survive_connection_changes_and_keep_arrival_order() {
        let mut desktop = Desktop::new();
        desktop.environment = Some("Local".into());
        for link in ["first", "second", "first"] {
            desktop.receive(WorkerMessage {
                id: None,
                reply: Reply::PairingOffer {
                    offer: Offer {
                        origin: "https://example.test".into(),
                        link: link.into(),
                    },
                },
            });
        }
        desktop.pairing = true;
        desktop.pending = true;
        desktop.receive(WorkerMessage {
            id: None,
            reply: Reply::LocalClosed,
        });
        assert!(desktop.pairing && desktop.pending);
        assert_eq!(
            desktop
                .pairing_offers
                .into_iter()
                .map(|offer| offer.link)
                .collect::<Vec<_>>(),
            ["first", "second"]
        );
    }
}
