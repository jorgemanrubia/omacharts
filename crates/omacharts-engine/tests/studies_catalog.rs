//! Every indicator the chart offers is in the studies' catalog, so whatever
//! walks that catalog sees every one of them.

use omacharts_engine::indicators::Kind;
use omacharts_studies::{Form, INDICATORS};

#[test]
fn every_indicator_is_in_the_catalog_and_nothing_else_is() {
    for kind in Kind::ALL {
        assert!(
            omacharts_studies::catalog::form(kind.key()).is_some(),
            "{} is not in omacharts_studies::INDICATORS — say how it can be computed",
            kind.key()
        );
    }
    for (key, _) in INDICATORS {
        assert!(Kind::ALL.iter().any(|kind| kind.key() == *key), "{key} is in the catalog but is no indicator");
    }
}

#[test]
fn a_stepped_indicator_is_computed_from_bars_alone() {
    // A study steps through bars; one that needs volume at price outright
    // cannot be one until its input is volume at price.
    for kind in Kind::ALL {
        if omacharts_studies::catalog::form(kind.key()) == Some(Form::Stepped) {
            assert_eq!(kind.needs(), omacharts_engine::indicators::Needs::Bars, "{}", kind.key());
        }
    }
}

#[test]
fn a_developing_study_develops_an_indicator_the_chart_draws_whole() {
    for (study, indicator) in omacharts_studies::DEVELOPING {
        assert!(
            Kind::ALL.iter().any(|kind| kind.key() == *indicator),
            "{study} develops {indicator}, which is no indicator"
        );
        assert_eq!(omacharts_studies::catalog::form(indicator), Some(Form::WholePeriod), "{study}");
    }
}
