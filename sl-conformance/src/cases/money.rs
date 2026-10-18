//! A census of money: what a grid answers a balance request, a gift, a payment
//! to an object, its pay-button layout and a purchase of an object set for
//! sale — what it refuses and how it says so, what it charges and tells each
//! side, and who holds what after.
//!
//! Avatar **A** pays and sells; avatar **B** is paid and buys. Both stand at
//! the fixtures' build location (Second Life) or the Default Region's centre
//! (OpenSim). One question a leg:
//!
//! 1. **Balances**: each avatar asks for its balance (`MoneyBalanceRequest`).
//! 2. **Gifts** (`MoneyTransferRequest`, type 5001): A gives B L$ 1 and B
//!    gives it back; A gives B L$ 0, A gives itself L$ 1, A gives an id that
//!    is nobody's L$ 1, and B gives A one more than it holds.
//! 3. **Paying an object** (type 5008): B asks a plain cube of A's for its
//!    pay buttons (`RequestPayPrice`) and pays it L$ 1; then a probe script
//!    that sets the buttons (`llSetPayPrice`) and says each `money` event to
//!    its owner goes into the cube, and B asks and pays again.
//! 4. **Buying a copy** (`ObjectBuy`): B buys a cube of A's that is not for
//!    sale; A sets it for sale as a copy while its next owner may not copy it,
//!    then lets the next owner copy, and B buys a copy for L$ 0 and for L$ 1,
//!    and at the wrong price and as the wrong kind of sale, each once for
//!    L$ 0 and once for more (OpenSim refuses any price but L$ 0 before it
//!    looks at the object).
//! 5. **Buying the contents**: A sells the cube's contents (a script) for
//!    L$ 0; B buys them, and buys the script alone (`BuyObjectInventory`).
//! 6. **Buying the original**: B buys one cube of A's for L$ 0 and another
//!    for L$ 1.
//!
//! Every leg records what each avatar was told (`*_told_a`, `*_told_b`: alerts,
//! instant messages, balance replies with their transactions, pay-button
//! layouts, the probe's lines, inventory pushes and unread event-queue
//! messages), and a purchase of an original whose the cube is after
//! (`*_owner`). A balance is recorded only as zero or not, and every id and
//! name is described by whose it is, so no metric names an avatar or an
//! amount held. The run ends with A giving B whatever B came to be short, so
//! the accounts hold what they did before.
//!
//! What each live grid answered is stated as a [`Measured`] and written up in
//! `book/src/gridspec/economy.md`; every run is held to it.
//!
//! `2av`, live grids.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use sl_client_tokio::{
    ChatSource, Command, DeRezDestination, Diagnostic, Event, GroupKey, InventoryFolderKey,
    InventoryKey, LindenAmount, MoneyBalance, MoneyTransactionType, Object, ObjectBuyItem,
    ObjectKey, PermissionField, Permissions, PrimShape, RegionLocalObjectId, RestoreItem,
    RezScriptParams, SaleType, ScopedObjectId, ScriptTarget, ScriptUploadLocation, TransactionId,
    Uuid, Vector,
};

use crate::cases::asset_http::without_ids;
use crate::cases::asset_upload::current_balance;
use crate::cases::groups::names_of;
use crate::cases::land_transactions::{region_named, teleport};
use crate::cases::object_edit::hex;
use crate::cases::object_rez_derez::{Answer, Folders, REZ_LIFT_M, SETTLE_IDLE, SETTLE_WINDOW};
use crate::cases::task_inventory::{OPENSIM_START, hold_to_measured, listing, login_folders};
use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, is_opensim, settle_scene, wait_for_own_new_object};

/// How long both avatars are listened to after a leg's request.
const WINDOW: Duration = Duration::from_secs(8);

/// How long a cube may take to appear, and a script to be made and saved.
const STEP: Duration = Duration::from_secs(30);

/// How far apart the census's cubes stand, in metres.
const SPACING_M: f32 = 1.5;

/// What every line the probe says begins with.
const PROBE_TAG: &str = "SLCMONEY";

/// The name of the probe script in the cube.
const PROBE_NAME: &str = "SLClientMoneyProbe";

/// The name of the plain script sold as the contents of a cube.
const WARE_NAME: &str = "SLClientMoneyWare";

/// The default amount the probe's pay buttons offer.
const PROBE_DEFAULT: i32 = 7;

/// The amount of every paid gift, payment and sale, in L$.
const PRICE: u64 = 1;

/// The description of every transfer the census sends.
const DESCRIPTION: &str = "sl-conformance money census";

/// Where the measurements below are written down.
const SOURCE: &str = "book/src/gridspec/economy.md (money, 2026-10-18)";

/// An answer both grids give.
const fn both(answer: Answer) -> Measured<Answer> {
    Measured {
        second_life: answer,
        opensim: answer,
        source: SOURCE,
    }
}

/// An answer the grids differ in.
const fn each(second_life: Answer, opensim: Answer) -> Measured<Answer> {
    Measured {
        second_life,
        opensim,
        source: SOURCE,
    }
}

/// What each grid was measured to answer, by the metric the census records it
/// under: every run is held to it.
const MEASURED: &[(&str, Measured<Answer>)] = &[
    (
        "b_short_after",
        each(Answer::Text("short"), Answer::Text("even")),
    ),
    (
        "balance_a_told_a",
        each(
            Answer::Text("balance not zero, no transaction \"\""),
            Answer::Text("balance zero, no transaction \"\""),
        ),
    ),
    ("balance_a_told_b", both(Answer::Text("nothing"))),
    ("balance_b_told_a", both(Answer::Text("nothing"))),
    (
        "balance_b_told_b",
        each(
            Answer::Text("balance not zero, no transaction \"\""),
            Answer::Text("balance zero, no transaction \"\""),
        ),
    ),
    (
        "buy_contents_told_a",
        each(
            Answer::Text(
                "balance with a transaction id not zero, type 5000 L$ 0 from B to A item \"Object\" \"\"",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "buy_contents_told_b",
        each(
            Answer::Text(
                "balance with a transaction id not zero, type 5000 L$ 0 from B to A item \"Object\" \"You paid <A> L$0 for Object.\"; bulk update: [folder \"Object\" type -1 in B's objects folder]",
            ),
            Answer::Text(
                "descendents of B's root folder; descendents of a folder it was not given before: [\"SLClientMoneyWare\" type 10 in a folder it was just given, owned by B, next owner 0x7fffffff]",
            ),
        ),
    ),
    (
        "buy_copy_free_told_a",
        each(
            Answer::Text(
                "balance with a transaction id not zero, type 5000 L$ 0 from B to A item \"Object\" \"\"",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "buy_copy_free_told_b",
        each(
            Answer::Text(
                "balance with a transaction id not zero, type 5000 L$ 0 from B to A item \"Object\" \"You paid <A> L$0 for Object.\"; item created: \"Object\" type 6 in B's objects folder, owned by B, next owner 0x0008e000",
            ),
            Answer::Text(
                "item created: \"Object\" type 6 in B's objects folder, owned by B, next owner 0x0008e000",
            ),
        ),
    ),
    (
        "buy_copy_no_copy_told_a",
        each(
            Answer::Text(
                "balance with a transaction id not zero, type 5000 L$ 0 from B to A item \"Object\" \"\"",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "buy_copy_no_copy_told_b",
        each(
            Answer::Text(
                "balance with a transaction id not zero, type 5000 L$ 0 from B to A item \"Object\" \"You paid <A> L$0 for Object.\"; item created: \"Object\" type 6 in B's objects folder, owned by B, next owner 0x00082000",
            ),
            Answer::Text(
                "item created: \"Object\" type 6 in B's objects folder, owned by B, next owner 0x00082000",
            ),
        ),
    ),
    (
        "buy_copy_paid_told_a",
        each(
            Answer::Text(
                "balance with a transaction id not zero, type 5000 L$ 1 from B to A item \"Object\" \"<B> paid you L$1 for Object.\"",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "buy_copy_paid_told_b",
        each(
            Answer::Text(
                "balance with a transaction id not zero, type 5000 L$ 1 from B to A item \"Object\" \"You paid <A> L$1 for Object.\"; item created: \"Object\" type 6 in B's objects folder, owned by B, next owner 0x0008e000",
            ),
            Answer::Text(
                "im MessageBox from nobody as \"\" \"Buying anything for a price other than zero is not implemented\"",
            ),
        ),
    ),
    ("buy_not_for_sale_told_a", both(Answer::Text("nothing"))),
    (
        "buy_not_for_sale_told_b",
        each(
            Answer::Text("nothing"),
            Answer::Text("agent alert \"/Object Object is not for sale\""),
        ),
    ),
    ("buy_object_inventory_told_a", both(Answer::Text("nothing"))),
    (
        "buy_object_inventory_told_b",
        each(
            Answer::Text("alert \"Inventory is not for sale.\" keyed InventoryNotForSale []"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "buy_original_free_owner",
        both(Answer::Text(
            "owned by B, last owned by A, sale type 0 at L$ 0, next owner 0x0008e000",
        )),
    ),
    (
        "buy_original_free_told_a",
        each(
            Answer::Text(
                "balance with a transaction id not zero, type 5000 L$ 0 from B to A item \"Object\" \"\"",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "buy_original_free_told_b",
        each(
            Answer::Text(
                "alert \"You are now the owner of object Object\" keyed NowOwnObject [<? LLSD/XML ?> <llsd><map><key>OBJECT_NAME</key><string>Object</string></map></llsd>]; balance with a transaction id not zero, type 5000 L$ 0 from B to A item \"Object\" \"You paid <A> L$0 for Object.\"",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "buy_original_paid_owner",
        each(
            Answer::Text("owned by B, last owned by A, sale type 0 at L$ 0, next owner 0x0008e000"),
            Answer::Text("owned by A, last owned by A, sale type 1 at L$ 1, next owner 0x0008e000"),
        ),
    ),
    (
        "buy_original_paid_told_a",
        each(
            Answer::Text(
                "balance with a transaction id not zero, type 5000 L$ 1 from B to A item \"Object\" \"<B> paid you L$1 for Object.\"",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "buy_original_paid_told_b",
        each(
            Answer::Text(
                "alert \"You are now the owner of object Object\" keyed NowOwnObject [<? LLSD/XML ?> <llsd><map><key>OBJECT_NAME</key><string>Object</string></map></llsd>]; balance with a transaction id not zero, type 5000 L$ 1 from B to A item \"Object\" \"You paid <A> L$1 for Object.\"",
            ),
            Answer::Text(
                "im MessageBox from nobody as \"\" \"Buying anything for a price other than zero is not implemented\"",
            ),
        ),
    ),
    (
        "buy_wrong_price_cheap_told_a",
        both(Answer::Text("nothing")),
    ),
    (
        "buy_wrong_price_cheap_told_b",
        each(
            Answer::Text("alert \"Sale of Object cancelled.  Price does not match expected.\""),
            Answer::Text("agent alert \"/Object Object price does not match selected price\""),
        ),
    ),
    ("buy_wrong_price_dear_told_a", both(Answer::Text("nothing"))),
    (
        "buy_wrong_price_dear_told_b",
        each(
            Answer::Text("alert \"Sale of Object cancelled.  Price does not match expected.\""),
            Answer::Text(
                "im MessageBox from nobody as \"\" \"Buying anything for a price other than zero is not implemented\"",
            ),
        ),
    ),
    ("buy_wrong_type_dear_told_a", both(Answer::Text("nothing"))),
    (
        "buy_wrong_type_dear_told_b",
        each(
            Answer::Text(
                "alert \"Sale of Object cancelled.  Sell type (copy,original,contents) does not match expected.\"",
            ),
            Answer::Text(
                "im MessageBox from nobody as \"\" \"Buying anything for a price other than zero is not implemented\"",
            ),
        ),
    ),
    ("buy_wrong_type_told_a", both(Answer::Text("nothing"))),
    (
        "buy_wrong_type_told_b",
        each(
            Answer::Text(
                "alert \"Sale of Object cancelled.  Sell type (copy,original,contents) does not match expected.\"",
            ),
            Answer::Text("agent alert \"/Object Object sell type does not match selected type\""),
        ),
    ),
    (
        "gift_back_told_a",
        each(
            Answer::Text(
                "balance with a transaction id not zero, type 5001 L$ 1 from B to A item \"sl-conformance money census\" \"<B> paid you L$1.\"",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "gift_back_told_b",
        each(
            Answer::Text(
                "balance with a transaction id not zero, type 5001 L$ 1 from B to A item \"sl-conformance money census\" \"You paid <A> L$1.\"",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "gift_nobody_told_a",
        each(
            Answer::Text(
                "balance (failed) with a transaction id not zero, type 5001 L$ 1 from A to the made-up id item \"sl-conformance money census\" \"There has been a problem transferring L$. This error has been logged and reported.\"",
            ),
            Answer::Text("nothing"),
        ),
    ),
    ("gift_nobody_told_b", both(Answer::Text("nothing"))),
    ("gift_over_balance_told_a", both(Answer::Text("nothing"))),
    (
        "gift_over_balance_told_b",
        each(
            Answer::Text(
                "balance (failed) with a transaction id not zero, type 5001 L$ <more than B holds> from B to A item \"sl-conformance money census\" \"Insufficient funds.\"",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "gift_self_told_a",
        each(
            Answer::Text(
                "balance with a transaction id not zero, type 5001 L$ 1 from A to A item \"sl-conformance money census\" \"<A> paid you L$1.\"; balance with a transaction id not zero, type 5001 L$ 1 from A to A item \"sl-conformance money census\" \"You paid <A> L$1.\"",
            ),
            Answer::Text("nothing"),
        ),
    ),
    ("gift_self_told_b", both(Answer::Text("nothing"))),
    (
        "gift_told_a",
        each(
            Answer::Text(
                "balance with a transaction id not zero, type 5001 L$ 1 from A to B item \"sl-conformance money census\" \"You paid <B> L$1.\"",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "gift_told_b",
        each(
            Answer::Text(
                "balance with a transaction id not zero, type 5001 L$ 1 from A to B item \"sl-conformance money census\" \"<A> paid you L$1.\"",
            ),
            Answer::Text("nothing"),
        ),
    ),
    ("gift_zero_told_a", both(Answer::Text("nothing"))),
    ("gift_zero_told_b", both(Answer::Text("nothing"))),
    (
        "pay_plain_told_a",
        each(
            Answer::Text(
                "balance with a transaction id not zero, type 5008 L$ 1 from B to A item \"sl-conformance money census\" \"<B> paid you L$1.\"",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "pay_plain_told_b",
        each(
            Answer::Text(
                "balance with a transaction id not zero, type 5008 L$ 1 from B to A item \"sl-conformance money census\" \"You paid <A> L$1.\"",
            ),
            Answer::Text("nothing"),
        ),
    ),
    ("pay_price_plain_told_a", both(Answer::Text("nothing"))),
    (
        "pay_price_plain_told_b",
        each(
            Answer::Text("pay price of the tip cube: default -2, buttons [1, 5, 10, 20]"),
            Answer::Text("pay price of the tip cube: default -2, buttons [-2, -2, -2, -2]"),
        ),
    ),
    ("pay_price_scripted_told_a", both(Answer::Text("nothing"))),
    (
        "pay_price_scripted_told_b",
        each(
            Answer::Text("pay price of the tip cube: default 7, buttons [3, 5, -1, 20]"),
            Answer::Text("pay price of the tip cube: default 7, buttons [3, 5, -1, -2]"),
        ),
    ),
    (
        "pay_scripted_as_gift_told_a",
        each(
            Answer::Text(
                "balance with a transaction id not zero, type 5008 L$ 1 from B to A item \"sl-conformance money census\" \"<B> paid you L$1.\"; chat by the tip cube Owner from \"Object\" \"SLCMONEY|money|<B>|1\"",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "pay_scripted_as_gift_told_b",
        each(
            Answer::Text(
                "balance with a transaction id not zero, type 5008 L$ 1 from B to A item \"sl-conformance money census\" \"You paid <A> L$1.\"",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "pay_scripted_by_owner_told_a",
        each(
            Answer::Text(
                "balance with a transaction id not zero, type 5008 L$ 1 from A to A item \"sl-conformance money census\" \"<A> paid you L$1.\"; balance with a transaction id not zero, type 5008 L$ 1 from A to A item \"sl-conformance money census\" \"You paid <A> L$1.\"; chat by the tip cube Owner from \"Object\" \"SLCMONEY|money|<A>|1\"",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "pay_scripted_by_owner_told_b",
        both(Answer::Text("nothing")),
    ),
    (
        "pay_scripted_told_a",
        each(
            Answer::Text(
                "balance with a transaction id not zero, type 5008 L$ 1 from B to A item \"sl-conformance money census\" \"<B> paid you L$1.\"; chat by the tip cube Owner from \"Object\" \"SLCMONEY|money|<B>|1\"",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "pay_scripted_told_b",
        each(
            Answer::Text(
                "balance with a transaction id not zero, type 5008 L$ 1 from B to A item \"sl-conformance money census\" \"You paid <A> L$1.\"",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "probe_started_told_a",
        both(Answer::Text(
            "chat by the tip cube Owner from \"Object\" \"SLCMONEY|ready\"",
        )),
    ),
    (
        "sell_copy_after",
        each(
            Answer::Text(
                "owned by A, last owned by nobody, sale type 2 at L$ 0, next owner 0x0008e000",
            ),
            Answer::Text("owned by A, last owned by A, sale type 2 at L$ 0, next owner 0x0008e000"),
        ),
    ),
    (
        "sell_copy_no_copy_after",
        each(
            Answer::Text(
                "owned by A, last owned by nobody, sale type 2 at L$ 0, next owner 0x00082000",
            ),
            Answer::Text("owned by A, last owned by A, sale type 2 at L$ 0, next owner 0x00082000"),
        ),
    ),
    ("sell_copy_no_copy_told_a", both(Answer::Text("nothing"))),
    ("sell_copy_no_copy_told_b", both(Answer::Text("nothing"))),
    (
        "ware_after_copies",
        each(
            Answer::Text(
                "owned by A, last owned by nobody, sale type 2 at L$ 1, next owner 0x0008e000",
            ),
            Answer::Text("owned by A, last owned by A, sale type 2 at L$ 1, next owner 0x0008e000"),
        ),
    ),
];

/// The probe: sets the pay buttons and says each payment to its owner, as
/// `SLCMONEY|<event>|<payer>|<amount>`.
fn probe_source() -> String {
    format!(
        "default\n\
         {{\n\
         \x20   state_entry()\n\
         \x20   {{\n\
         \x20       llSetPayPrice({PROBE_DEFAULT}, [3, 5, PAY_HIDE, PAY_DEFAULT]);\n\
         \x20       llOwnerSay(\"{PROBE_TAG}|ready\");\n\
         \x20   }}\n\
         \x20   money(key payer, integer amount)\n\
         \x20   {{\n\
         \x20       llOwnerSay(\"{PROBE_TAG}|money|\" + (string)payer + \"|\" + (string)amount);\n\
         \x20   }}\n\
         }}\n"
    )
}

/// The money census.
#[derive(Debug)]
pub struct Money;

impl GridTest for Money {
    fn name(&self) -> &'static str {
        "money"
    }

    fn description(&self) -> &'static str {
        "Balances, gifts, paying an object and its pay buttons, and buying an object's copy, \
         contents or original: refusals, charges and what each side is told"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn accounts(&self) -> u8 {
        2
    }

    fn start_location(&self, grid: Grid) -> &'static str {
        if is_opensim(grid) {
            OPENSIM_START
        } else {
            "last"
        }
    }

    fn rezzes_objects(&self) -> bool {
        true
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(1500)
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let a_folders = login_folders(ctx.primary()).await?;
            let b_folders = {
                let b = ctx.secondary().ok_or_else(|| {
                    TestFailure::State("money needs a secondary avatar".to_owned())
                })?;
                login_folders(b).await?
            };
            let mut metrics = Metrics::new();
            let outcome = census(ctx, &mut metrics, a_folders, b_folders).await;
            let grid = ctx.grid();
            ctx.metrics().merge(metrics.clone());
            outcome.and_then(|()| hold_to_measured(grid, &metrics, MEASURED, SOURCE))
        })
    }
}

/// Which avatar acts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Who {
    /// The payer and seller.
    A,
    /// The payee and buyer.
    B,
}

/// Who and what is what, for describing ids and scrubbing names.
#[derive(Debug, Clone, Default)]
struct Scene {
    /// Avatar A.
    a: Uuid,
    /// Avatar B.
    b: Uuid,
    /// The id the census gives money to that is nobody's.
    nobody: Uuid,
    /// The amount of the gift of more than B holds, which says what B holds.
    over: u64,
    /// The census's cubes, each with its word.
    cubes: Vec<(Uuid, &'static str)>,
    /// A's folders.
    a_folders: Option<Folders>,
    /// B's folders.
    b_folders: Option<Folders>,
    /// Every way a grid may write A's or B's name, with the word that
    /// replaces it.
    names: Vec<(String, &'static str)>,
}

impl Scene {
    /// Whose or what `id` is, in a word.
    fn whose(&self, id: Uuid) -> &'static str {
        if id.is_nil() {
            "nobody"
        } else if id == self.a {
            "A"
        } else if id == self.b {
            "B"
        } else if id == self.nobody {
            "the made-up id"
        } else if let Some((_id, word)) = self.cubes.iter().find(|(cube, _word)| *cube == id) {
            word
        } else {
            "another"
        }
    }

    /// The folder `folder` in a word, or `None` when it is none the census
    /// knows.
    fn folder(&self, folder: InventoryFolderKey) -> Option<&'static str> {
        for (folders, whose) in [(&self.a_folders, "A's"), (&self.b_folders, "B's")] {
            let Some(folders) = folders else {
                continue;
            };
            let word = if folder == folders.root {
                "root"
            } else if folder == folders.objects {
                "objects"
            } else if folder == folders.trash {
                "trash"
            } else if folder == folders.lost_and_found {
                "lost and found"
            } else {
                continue;
            };
            return Some(match (whose, word) {
                ("A's", "root") => "A's root folder",
                ("A's", "objects") => "A's objects folder",
                ("A's", "trash") => "A's trash",
                ("A's", _) => "A's lost and found",
                (_, "root") => "B's root folder",
                (_, "objects") => "B's objects folder",
                (_, "trash") => "B's trash",
                _ => "B's lost and found",
            });
        }
        None
    }

    /// `text` with A's and B's ids and names replaced, and every other id too.
    fn scrub(&self, text: &str) -> String {
        let mut out = text.to_owned();
        for (id, word) in [(self.a, "<A>"), (self.b, "<B>")] {
            if !id.is_nil() {
                out = out.replace(&id.to_string(), word);
            }
        }
        for (id, word) in &self.cubes {
            out = out.replace(&id.to_string(), &format!("<{word}>"));
        }
        out = without_ids(&out);
        for (name, word) in &self.names {
            if !name.is_empty() {
                out = out.replace(name.as_str(), word);
            }
        }
        out
    }
}

/// What one session was sent during a window: one line per message the census
/// reads, in arrival order, and the inventory the grid gave it.
#[derive(Debug, Default)]
struct Got {
    /// One line per message.
    lines: Vec<String>,
    /// Every inventory item pushed.
    items: Vec<InventoryKey>,
    /// Every inventory folder pushed.
    folders: Vec<InventoryFolderKey>,
}

/// A balance reply in words: whether it succeeded, whether the balance is
/// zero, the transaction it names and its description — never the balance.
fn describe_money(scene: &Scene, money: &MoneyBalance) -> String {
    let transaction = money.transaction.as_ref().map_or_else(
        || "no transaction".to_owned(),
        |transaction| {
            let amount = if transaction.amount.0 == scene.over && scene.over > PRICE {
                "<more than B holds>".to_owned()
            } else {
                transaction.amount.0.to_string()
            };
            format!(
                "type {} L$ {amount} from {} to {} item \"{}\"",
                transaction.transaction_type,
                scene.whose(transaction.source.uuid()),
                scene.whose(transaction.dest.uuid()),
                scene.scrub(&transaction.item_description)
            )
        },
    );
    format!(
        "balance{}{} {}, {transaction} \"{}\"",
        if money.success { "" } else { " (failed)" },
        if money.transaction_id.is_nil() {
            ""
        } else {
            " with a transaction id"
        },
        if money.balance.0 == 0 {
            "zero"
        } else {
            "not zero"
        },
        scene.scrub(&money.description)
    )
}

/// Folds one event into `got`; `None` for an event the census does not read.
fn fold(scene: &Scene, got: &mut Got, event: &Event) -> Option<()> {
    let line = match event {
        Event::AlertMessage {
            message,
            alert_info,
            ..
        } => {
            let keys = alert_info
                .iter()
                .map(|info| {
                    format!(
                        "{} [{}]",
                        info.message,
                        scene.scrub(&compact(&info.extra_params))
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            if keys.is_empty() {
                format!("alert \"{}\"", scene.scrub(message))
            } else {
                format!("alert \"{}\" keyed {keys}", scene.scrub(message))
            }
        }
        Event::AgentAlertMessage { message, modal, .. } => format!(
            "{} \"{}\"",
            if *modal {
                "modal agent alert"
            } else {
                "agent alert"
            },
            scene.scrub(message)
        ),
        Event::InstantMessageReceived(im) => format!(
            "im {:?} from {} as \"{}\" \"{}\"",
            im.dialog,
            scene.whose(im.from_agent_id.uuid()),
            scene.scrub(&im.from_agent_name),
            scene.scrub(&im.message)
        ),
        Event::ChatReceived(chat) if !chat.message.is_empty() => format!(
            "chat {} {:?} from \"{}\" \"{}\"",
            match &chat.source {
                ChatSource::System => "by the system".to_owned(),
                ChatSource::Agent(agent) => format!("by {}", scene.whose(agent.uuid())),
                ChatSource::Object(object) => format!("by {}", scene.whose(object.uuid())),
                ChatSource::Unknown { source_type, .. } => format!("by source {source_type}"),
            },
            chat.chat_type,
            scene.scrub(&chat.from_name),
            scene.scrub(&chat.message)
        ),
        Event::MoneyBalance(money) => describe_money(scene, money),
        Event::PayPriceReply {
            object_id,
            default_pay_price,
            pay_buttons,
        } => format!(
            "pay price of {}: default {default_pay_price}, buttons {pay_buttons:?}",
            scene.whose(object_id.uuid())
        ),
        Event::InventoryItemCreated { item, .. } => {
            got.items.push(item.item_id);
            format!("item created: {}", describe_item(scene, got, item))
        }
        Event::InventoryDescendents {
            folder_id, items, ..
        } => {
            if let Some(word) = scene.folder(*folder_id) {
                // A grid may send a folder's descendents in several messages,
                // as many as its contents take: one line says it was sent.
                let line = format!("descendents of {word}");
                if got.lines.contains(&line) {
                    return Some(());
                }
                line
            } else {
                got.folders.push(*folder_id);
                let listed = items
                    .iter()
                    .map(|item| {
                        got.items.push(item.item_id);
                        describe_item(scene, got, item)
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("descendents of a folder it was not given before: [{listed}]")
            }
        }
        Event::InventoryBulkUpdate { folders, items, .. } => {
            let mut parts = Vec::new();
            for folder in folders {
                got.folders.push(folder.folder_id);
                parts.push(format!(
                    "folder \"{}\" type {} in {}",
                    scene.scrub(&folder.name),
                    folder.folder_type,
                    folder
                        .parent_id
                        .and_then(|parent| scene.folder(parent))
                        .unwrap_or("another folder")
                ));
            }
            for item in items {
                got.items.push(item.item_id);
                parts.push(format!("item {}", describe_item(scene, got, item)));
            }
            format!("bulk update: [{}]", parts.join(", "))
        }
        _ => return None,
    };
    got.lines.push(line);
    Some(())
}

/// `text` on one line: every run of whitespace a single space.
fn compact(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// An inventory item in words: its name, kind, folder and next owner's mask.
fn describe_item(scene: &Scene, got: &Got, item: &sl_client_tokio::InventoryItem) -> String {
    let folder = scene.folder(item.folder_id).unwrap_or_else(|| {
        if got.folders.contains(&item.folder_id) {
            "a folder it was just given"
        } else {
            "another folder"
        }
    });
    format!(
        "\"{}\" type {} in {folder}, owned by {}, next owner {}",
        scene.scrub(&item.name),
        item.item_type,
        scene.whose(item.owner.uuid()),
        hex(item.permissions.next_owner)
    )
}

/// Listens on `session` for `window`.
async fn watch(scene: &Scene, session: &mut Session, window: Duration) -> Result<Got, TestFailure> {
    let mut got = Got::default();
    let started = Instant::now();
    loop {
        let remaining = window.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Ok(got);
        }
        match session
            .wait_for(remaining, |event| fold(scene, &mut got, event))
            .await
        {
            Ok(()) => {}
            Err(TestFailure::Timeout(_)) => return Ok(got),
            Err(other) => return Err(other),
        }
    }
}

/// The names of the event-queue messages `session` was sent and did not read
/// since it had seen `seen` diagnostics, and the new count.
fn unread_since(session: &Session, seen: usize) -> (Vec<String>, usize) {
    let all = session.diagnostics();
    let names = all
        .iter()
        .skip(seen)
        .filter_map(|diagnostic| match diagnostic {
            Diagnostic::UnknownCapsEvent { message } => Some(message.clone()),
            _ => None,
        })
        .collect();
    (names, all.len())
}

/// What `got` holds, in words, sorted — a script's line and a balance reply
/// arrive in either order — and `nothing` for an empty window.
fn describe_got(got: &Got, unread: &[String]) -> String {
    let mut parts = got.lines.clone();
    for name in unread {
        parts.push(format!("unread event {name}"));
    }
    parts.sort();
    if parts.is_empty() {
        "nothing".to_owned()
    } else {
        parts.join("; ")
    }
}

/// One of the census's cubes.
#[derive(Debug, Clone)]
struct Cube {
    /// The cube as A sees it.
    object: Object,
}

impl Cube {
    /// The cube's region-local id, scoped to A's circuit.
    const fn scoped(&self) -> ScopedObjectId {
        self.object.scoped_id()
    }

    /// The cube's full id.
    const fn id(&self) -> ObjectKey {
        self.object.full_id
    }

    /// One line of an `ObjectBuy` of the cube.
    const fn buy_item(&self, sale_type: SaleType, price: u64) -> ObjectBuyItem {
        ObjectBuyItem {
            local_id: self.object.local_id,
            sale_type,
            sale_price: LindenAmount(price),
        }
    }
}

/// The census's state: the two sessions, who is who and what is left to undo.
struct Census<'s> {
    /// Avatar A.
    a: &'s mut Session,
    /// Avatar B.
    b: &'s mut Session,
    /// Who and what is what.
    scene: Scene,
    /// How many diagnostics each session had shown when last looked at.
    seen: (usize, usize),
    /// The objects A saw before its cubes, none of which is one.
    objects_seen: HashSet<ScopedObjectId>,
    /// Where the first cube goes.
    cube_at: Vector,
    /// How many cubes have been rezzed.
    rezzed: u8,
    /// A's cubes still standing.
    a_cubes: Vec<ScopedObjectId>,
    /// The cubes B came to own, by region-local id.
    b_cubes: Vec<RegionLocalObjectId>,
    /// The items B was given.
    b_items: Vec<InventoryKey>,
    /// The folders B was given.
    b_folders: Vec<InventoryFolderKey>,
}

impl Census<'_> {
    /// The session of `who`.
    const fn session(&mut self, who: Who) -> &mut Session {
        match who {
            Who::A => self.a,
            Who::B => self.b,
        }
    }

    /// Lets both avatars hear out what the grid still had to say, so it is not
    /// taken for the next leg's answer.
    async fn settle(&mut self) -> Result<(), TestFailure> {
        let scene = self.scene.clone();
        let (got_a, got_b) =
            tokio::join!(watch(&scene, self.a, WINDOW), watch(&scene, self.b, WINDOW));
        let (_got_a, got_b) = (got_a?, got_b?);
        self.b_items.extend(got_b.items);
        self.b_folders.extend(got_b.folders);
        let (_unread, seen_a) = unread_since(self.a, self.seen.0);
        let (_unread, seen_b) = unread_since(self.b, self.seen.1);
        self.seen = (seen_a, seen_b);
        Ok(())
    }

    /// Sends `commands` as `who`, listens to both avatars and records what
    /// each was told under `key`.
    async fn leg(
        &mut self,
        metrics: &mut Metrics,
        key: &str,
        who: Who,
        commands: Vec<Command>,
    ) -> Result<(Got, Got), TestFailure> {
        for command in commands {
            self.session(who).send(command).await?;
        }
        let scene = self.scene.clone();
        let (got_a, got_b) =
            tokio::join!(watch(&scene, self.a, WINDOW), watch(&scene, self.b, WINDOW));
        let (got_a, got_b) = (got_a?, got_b?);
        let (unread_a, seen_a) = unread_since(self.a, self.seen.0);
        let (unread_b, seen_b) = unread_since(self.b, self.seen.1);
        self.seen = (seen_a, seen_b);
        metrics.set(&format!("{key}_told_a"), describe_got(&got_a, &unread_a));
        metrics.set(&format!("{key}_told_b"), describe_got(&got_b, &unread_b));
        self.b_items.extend(got_b.items.iter().copied());
        self.b_folders.extend(got_b.folders.iter().copied());
        Ok((got_a, got_b))
    }

    /// A gift of `amount` L$ from `who` to `dest`, recorded under `key`.
    async fn give(
        &mut self,
        metrics: &mut Metrics,
        key: &str,
        who: Who,
        dest: Uuid,
        amount: u64,
        kind: MoneyTransactionType,
    ) -> Result<(), TestFailure> {
        self.leg(
            metrics,
            key,
            who,
            vec![Command::SendMoneyTransfer {
                dest,
                amount: LindenAmount(amount),
                kind,
                description: DESCRIPTION.to_owned(),
            }],
        )
        .await?;
        Ok(())
    }

    /// Rezzes a cube as A, named `word` in the metrics.
    async fn rez(&mut self, word: &'static str) -> Result<Cube, TestFailure> {
        let at = Vector {
            x: f32::from(self.rezzed).mul_add(SPACING_M, self.cube_at.x),
            y: self.cube_at.y,
            z: self.cube_at.z,
        };
        self.rezzed = self.rezzed.saturating_add(1);
        self.a
            .send(Command::RezObject {
                shape: PrimShape::cube(at),
                group_id: None,
            })
            .await?;
        let object = wait_for_own_new_object(self.a, &self.objects_seen, STEP)
            .await?
            .map_err(|reason| {
                TestFailure::Assertion(format!(
                    "no cube appeared after RezObject: {}",
                    self.scene.scrub(&reason)
                ))
            })?;
        self.objects_seen.insert(object.scoped_id());
        self.a_cubes.push(object.scoped_id());
        self.scene.cubes.push((object.full_id.uuid(), word));
        self.settle().await?;
        Ok(Cube { object })
    }

    /// Makes a new script named `name` in `cube` as A and returns its item,
    /// once the cube's contents list it.
    async fn new_script(&mut self, cube: &Cube, name: &str) -> Result<InventoryKey, TestFailure> {
        let owner = self.scene.a;
        self.a
            .send(Command::RezScript {
                target: cube.scoped(),
                params: Box::new(RezScriptParams {
                    group_id: None,
                    enabled: true,
                    item: RestoreItem::new_script(owner.into(), cube.id(), name, Uuid::new_v4()),
                }),
            })
            .await?;
        let started = Instant::now();
        loop {
            let scene = self.scene.clone();
            let _heard = watch(&scene, self.a, Duration::from_millis(500)).await?;
            let held = listing(self.a, cube.scoped(), cube.id()).await?;
            if let Some(item) = held.items.iter().find(|item| item.name == name) {
                return Ok(item.item_id);
            }
            if started.elapsed() >= STEP {
                return Err(TestFailure::Assertion(
                    "a new script never appeared in the cube".to_owned(),
                ));
            }
        }
    }

    /// Saves the probe over the script `item` of `cube`, to run, and listens
    /// until it has said it started; records what A heard under `key`.
    async fn save_probe(
        &mut self,
        metrics: &mut Metrics,
        key: &str,
        cube: &Cube,
        item: InventoryKey,
    ) -> Result<(), TestFailure> {
        self.a
            .send(Command::UploadScript {
                location: ScriptUploadLocation::TaskInventory {
                    task_id: cube.id(),
                    item_id: item,
                    running: true,
                    experience: None,
                },
                target: ScriptTarget::Mono,
                source: probe_source().into_bytes(),
            })
            .await?;
        // The script may speak before the capability's answer is read, so one
        // window hears both.
        let scene = self.scene.clone();
        let mut compiled: Option<Result<bool, String>> = None;
        let mut got = Got::default();
        let started = Instant::now();
        loop {
            let ready = compiled.is_some()
                && got
                    .lines
                    .iter()
                    .any(|line| line.contains(&format!("{PROBE_TAG}|ready")));
            let remaining = STEP.saturating_sub(started.elapsed());
            if ready || remaining.is_zero() {
                break;
            }
            let outcome = self
                .a
                .wait_for(remaining, |event| match event {
                    Event::ScriptUploaded { compiled: took, .. } => {
                        compiled = Some(Ok(*took));
                        Some(())
                    }
                    Event::AssetUploadFailed { reason } => {
                        compiled = Some(Err(reason.clone()));
                        Some(())
                    }
                    other => fold(&scene, &mut got, other),
                })
                .await;
            match outcome {
                Ok(()) => {}
                Err(TestFailure::Timeout(_)) => break,
                Err(other) => return Err(other),
            }
        }
        match compiled {
            Some(Ok(true)) => {}
            Some(Ok(false)) => {
                return Err(TestFailure::Assertion(
                    "the probe script did not compile".to_owned(),
                ));
            }
            Some(Err(reason)) => {
                return Err(TestFailure::Assertion(format!(
                    "saving the probe script failed: {reason}"
                )));
            }
            None => {
                return Err(TestFailure::Assertion(
                    "saving the probe script was never answered".to_owned(),
                ));
            }
        }
        metrics.set(key, describe_got(&got, &[]));
        self.settle().await
    }

    /// Puts `cube` up for sale as `sale_type` at `price`, as A.
    async fn sell(
        &mut self,
        cube: &Cube,
        sale_type: SaleType,
        price: u64,
    ) -> Result<(), TestFailure> {
        self.a
            .send(Command::SetObjectForSale {
                local_id: cube.scoped(),
                sale_type,
                sale_price: Some(LindenAmount(price)),
            })
            .await?;
        self.settle().await
    }

    /// Lets or keeps the next owner of `cube` from `mask`, as A.
    async fn next_owner(
        &mut self,
        cube: &Cube,
        set: bool,
        mask: Permissions,
    ) -> Result<(), TestFailure> {
        self.a
            .send(Command::SetObjectPermissions {
                local_ids: vec![cube.scoped()],
                field: PermissionField::NextOwner,
                set,
                mask,
            })
            .await?;
        self.settle().await
    }

    /// A purchase by B of `item`, into B's objects folder.
    fn buy(&self, item: ObjectBuyItem) -> Command {
        Command::BuyObject {
            group_id: GroupKey::from(Uuid::nil()),
            category_id: self
                .scene
                .b_folders
                .map_or_else(Uuid::nil, |folders| folders.objects.uuid()),
            objects: vec![item],
        }
    }

    /// `cube`'s owner and sale, in words, as A reads it.
    async fn read(&mut self, cube: &Cube) -> Result<String, TestFailure> {
        let object_id = cube.id();
        self.a
            .send(Command::RequestObjectPropertiesFamily {
                request_flags: 0,
                object_id,
            })
            .await?;
        let scene = self.scene.clone();
        let answer = self
            .a
            .wait_for(WINDOW, |event| match event {
                Event::ObjectPropertiesFamily { properties }
                    if properties.object_id == object_id =>
                {
                    Some(format!(
                        "owned by {}, last owned by {}, sale type {} at L$ {}, next owner {}",
                        scene.whose(properties.owner.uuid()),
                        scene.whose(properties.last_owner_id),
                        properties.sale_type,
                        properties.sale_price.as_ref().map_or(0, |price| price.0),
                        hex(properties.permissions.next_owner)
                    ))
                }
                _ => None,
            })
            .await;
        match answer {
            Ok(answer) => Ok(answer),
            Err(TestFailure::Timeout(_)) => Ok("no answer".to_owned()),
            Err(other) => Err(other),
        }
    }
}

/// Logs both avatars in at the spot and runs the legs, then puts back what
/// they changed.
async fn census(
    ctx: &mut TestContext,
    metrics: &mut Metrics,
    a_folders: Folders,
    b_folders: Folders,
) -> Result<(), TestFailure> {
    let grid = ctx.grid();
    let build_point = ctx.build_position();
    let somewhere = ctx
        .build_region()
        .zip(ctx.build_position())
        .map(|(region, point)| (region.to_owned(), point));
    let Some((a, b)) = ctx.primary_and_secondary() else {
        return Err(TestFailure::State(
            "money needs a secondary avatar".to_owned(),
        ));
    };
    for session in [&mut *a, &mut *b] {
        session.wait_for_region(REGION_TIMEOUT).await?;
    }
    let id = |session: &Session| {
        session
            .agent_id()
            .map(|agent| agent.uuid())
            .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))
    };
    let (a_id, b_id) = (id(a)?, id(b)?);
    let mut names: Vec<(String, &'static str)> = Vec::new();
    for (session, word) in [(&*a, "<A>"), (&*b, "<B>")] {
        for name in names_of(&session.avatar_legacy_name()) {
            names.push((name, word));
        }
    }
    // The longest first, so a legacy name is not half-replaced by its first
    // name.
    names.sort_by_key(|(name, _word)| std::cmp::Reverse(name.len()));
    if !is_opensim(grid) {
        let (region, point) = somewhere.ok_or_else(|| {
            TestFailure::State("money needs a build_location in the fixtures".to_owned())
        })?;
        // The map lookup reads past the login's object stream, own avatar
        // and all, which `settle_scene` needs: look only when an avatar is
        // elsewhere. A teleport brings a fresh stream with it.
        let here = |session: &Session| {
            session
                .region_identity()
                .and_then(|identity| identity.sim_name.as_ref())
                .is_some_and(|name| name.to_string().eq_ignore_ascii_case(&region))
        };
        if !(here(a) && here(b)) {
            let handle = region_named(a, &region).await?;
            for session in [&mut *a, &mut *b] {
                teleport(session, handle, &point).await?;
            }
        }
    }
    let (objects_seen, anchor) =
        settle_scene(a, grid, build_point, SETTLE_WINDOW, SETTLE_IDLE).await?;
    let anchor = anchor.ok_or_else(|| {
        TestFailure::Assertion("the arrival streamed nothing to place a cube against".to_owned())
    })?;
    // B's balance before the census, so the census can make it whole after.
    let b_before = current_balance(b).await?;
    let mut census = Census {
        a,
        b,
        scene: Scene {
            a: a_id,
            b: b_id,
            nobody: Uuid::new_v4(),
            over: over_balance(b_before),
            cubes: Vec::new(),
            a_folders: Some(a_folders),
            b_folders: Some(b_folders),
            names,
        },
        seen: (0, 0),
        objects_seen,
        cube_at: Vector {
            x: anchor.x,
            y: anchor.y,
            z: anchor.z + REZ_LIFT_M,
        },
        rezzed: 0,
        a_cubes: Vec::new(),
        b_cubes: Vec::new(),
        b_items: Vec::new(),
        b_folders: Vec::new(),
    };
    census.settle().await?;
    let outcome = legs(&mut census, metrics).await;
    let cleaned = clean_up(&mut census, a_folders.trash, b_folders.trash).await;
    let squared = square(&mut census, metrics, b_before).await;
    outcome.and(cleaned).and(squared)
}

/// One more than `balance`, the most a transfer can carry at most.
fn over_balance(balance: i64) -> u64 {
    u64::try_from(balance.saturating_add(1))
        .unwrap_or(1)
        .min(u64::from(i32::MAX.unsigned_abs()))
}

/// Every leg, in order.
#[expect(
    clippy::too_many_lines,
    reason = "one linear flow of legs, each a request and a window to hear its answer in"
)]
async fn legs(census: &mut Census<'_>, metrics: &mut Metrics) -> Result<(), TestFailure> {
    let (a, b, nobody) = (census.scene.a, census.scene.b, census.scene.nobody);

    // 1. Balances.
    census
        .leg(
            metrics,
            "balance_a",
            Who::A,
            vec![Command::RequestMoneyBalance],
        )
        .await?;
    census
        .leg(
            metrics,
            "balance_b",
            Who::B,
            vec![Command::RequestMoneyBalance],
        )
        .await?;

    // 2. Gifts.
    census
        .give(
            metrics,
            "gift",
            Who::A,
            b,
            PRICE,
            MoneyTransactionType::Gift,
        )
        .await?;
    census
        .give(
            metrics,
            "gift_back",
            Who::B,
            a,
            PRICE,
            MoneyTransactionType::Gift,
        )
        .await?;
    census
        .give(
            metrics,
            "gift_zero",
            Who::A,
            b,
            0,
            MoneyTransactionType::Gift,
        )
        .await?;
    census
        .give(
            metrics,
            "gift_self",
            Who::A,
            a,
            PRICE,
            MoneyTransactionType::Gift,
        )
        .await?;
    census
        .give(
            metrics,
            "gift_nobody",
            Who::A,
            nobody,
            PRICE,
            MoneyTransactionType::Gift,
        )
        .await?;
    let over = census.scene.over;
    census
        .give(
            metrics,
            "gift_over_balance",
            Who::B,
            a,
            over,
            MoneyTransactionType::Gift,
        )
        .await?;

    // 3. Paying an object: a plain cube, then the probe in it.
    let tip = census.rez("the tip cube").await?;
    census
        .leg(
            metrics,
            "pay_price_plain",
            Who::B,
            vec![Command::RequestPayPrice {
                object_id: tip.id(),
            }],
        )
        .await?;
    census
        .give(
            metrics,
            "pay_plain",
            Who::B,
            tip.id().uuid(),
            PRICE,
            MoneyTransactionType::PayObject,
        )
        .await?;
    let probe = census.new_script(&tip, PROBE_NAME).await?;
    census
        .save_probe(metrics, "probe_started_told_a", &tip, probe)
        .await?;
    census
        .leg(
            metrics,
            "pay_price_scripted",
            Who::B,
            vec![Command::RequestPayPrice {
                object_id: tip.id(),
            }],
        )
        .await?;
    census
        .give(
            metrics,
            "pay_scripted",
            Who::B,
            tip.id().uuid(),
            PRICE,
            MoneyTransactionType::PayObject,
        )
        .await?;
    census
        .give(
            metrics,
            "pay_scripted_as_gift",
            Who::B,
            tip.id().uuid(),
            PRICE,
            MoneyTransactionType::Gift,
        )
        .await?;
    census
        .give(
            metrics,
            "pay_scripted_by_owner",
            Who::A,
            tip.id().uuid(),
            PRICE,
            MoneyTransactionType::PayObject,
        )
        .await?;

    // 4. Buying a copy.
    let ware = census.rez("the ware cube").await?;
    let ware_script = census.new_script(&ware, WARE_NAME).await?;
    census.settle().await?;
    let not_for_sale = census.buy(ware.buy_item(SaleType::Original, 0));
    census
        .leg(metrics, "buy_not_for_sale", Who::B, vec![not_for_sale])
        .await?;
    census.next_owner(&ware, false, Permissions::COPY).await?;
    census
        .leg(
            metrics,
            "sell_copy_no_copy",
            Who::A,
            vec![Command::SetObjectForSale {
                local_id: ware.scoped(),
                sale_type: SaleType::Copy,
                sale_price: Some(LindenAmount(0)),
            }],
        )
        .await?;
    let read = census.read(&ware).await?;
    metrics.set("sell_copy_no_copy_after", read);
    let copy_no_copy = census.buy(ware.buy_item(SaleType::Copy, 0));
    census
        .leg(metrics, "buy_copy_no_copy", Who::B, vec![copy_no_copy])
        .await?;
    census
        .next_owner(
            &ware,
            true,
            Permissions::COPY
                .union(Permissions::MODIFY)
                .union(Permissions::TRANSFER),
        )
        .await?;
    census.sell(&ware, SaleType::Copy, 0).await?;
    let read = census.read(&ware).await?;
    metrics.set("sell_copy_after", read);
    let copy_free = census.buy(ware.buy_item(SaleType::Copy, 0));
    census
        .leg(metrics, "buy_copy_free", Who::B, vec![copy_free])
        .await?;
    // A purchase at the wrong price or as the wrong kind, each for L$ 0 or
    // for more: OpenSim refuses anything but L$ 0 before it looks at the
    // object at all.
    let wrong_type = census.buy(ware.buy_item(SaleType::Original, 0));
    census
        .leg(metrics, "buy_wrong_type", Who::B, vec![wrong_type])
        .await?;
    let wrong_price_dear = census.buy(ware.buy_item(SaleType::Copy, PRICE));
    census
        .leg(
            metrics,
            "buy_wrong_price_dear",
            Who::B,
            vec![wrong_price_dear],
        )
        .await?;
    census.sell(&ware, SaleType::Copy, PRICE).await?;
    let copy_paid = census.buy(ware.buy_item(SaleType::Copy, PRICE));
    census
        .leg(metrics, "buy_copy_paid", Who::B, vec![copy_paid])
        .await?;
    let wrong_price_cheap = census.buy(ware.buy_item(SaleType::Copy, 0));
    census
        .leg(
            metrics,
            "buy_wrong_price_cheap",
            Who::B,
            vec![wrong_price_cheap],
        )
        .await?;
    let wrong_type_dear = census.buy(ware.buy_item(SaleType::Original, PRICE));
    census
        .leg(
            metrics,
            "buy_wrong_type_dear",
            Who::B,
            vec![wrong_type_dear],
        )
        .await?;
    let read = census.read(&ware).await?;
    metrics.set("ware_after_copies", read);

    // 5. Buying the contents.
    census.sell(&ware, SaleType::Contents, 0).await?;
    let contents = census.buy(ware.buy_item(SaleType::Contents, 0));
    census
        .leg(metrics, "buy_contents", Who::B, vec![contents])
        .await?;
    let b_objects = census
        .scene
        .b_folders
        .map(|folders| folders.objects)
        .ok_or_else(|| TestFailure::State("no folders of B's".to_owned()))?;
    census
        .leg(
            metrics,
            "buy_object_inventory",
            Who::B,
            vec![Command::BuyObjectInventory {
                object_id: ware.id(),
                item_id: ware_script,
                folder_id: b_objects,
            }],
        )
        .await?;

    // 6. Buying the original.
    for (word, key, price) in [
        ("the free cube", "buy_original_free", 0),
        ("the dear cube", "buy_original_paid", PRICE),
    ] {
        let cube = census.rez(word).await?;
        census
            .next_owner(
                &cube,
                true,
                Permissions::COPY
                    .union(Permissions::MODIFY)
                    .union(Permissions::TRANSFER),
            )
            .await?;
        census.sell(&cube, SaleType::Original, price).await?;
        let original = census.buy(cube.buy_item(SaleType::Original, price));
        census.leg(metrics, key, Who::B, vec![original]).await?;
        let read = census.read(&cube).await?;
        if read.starts_with("owned by B") {
            census.a_cubes.retain(|scoped| *scoped != cube.scoped());
            census.b_cubes.push(cube.object.local_id);
        }
        metrics.set(&format!("{key}_owner"), read);
    }
    Ok(())
}

/// Takes the cubes away — A's to A's Trash, B's to B's — and the inventory
/// B was given out of B's inventory.
async fn clean_up(
    census: &mut Census<'_>,
    a_trash: InventoryFolderKey,
    b_trash: InventoryFolderKey,
) -> Result<(), TestFailure> {
    census.settle().await?;
    let mut left = Vec::new();
    if !census.a_cubes.is_empty() {
        census
            .a
            .send(Command::DerezObjects {
                local_ids: census.a_cubes.clone(),
                destination: DeRezDestination::Trash(a_trash),
                transaction_id: TransactionId::from(Uuid::new_v4()),
                group_id: None,
            })
            .await?;
        left.extend(census.a_cubes.iter().map(|scoped| (Who::A, *scoped)));
    }
    if !census.b_cubes.is_empty() {
        let circuit = census
            .b
            .circuit_id()
            .ok_or_else(|| TestFailure::State("B has no root circuit".to_owned()))?;
        let scoped: Vec<ScopedObjectId> = census
            .b_cubes
            .iter()
            .map(|local_id| ScopedObjectId::new(circuit, *local_id))
            .collect();
        {
            census
                .b
                .send(Command::DerezObjects {
                    local_ids: scoped.clone(),
                    destination: DeRezDestination::Trash(b_trash),
                    transaction_id: TransactionId::from(Uuid::new_v4()),
                    group_id: None,
                })
                .await?;
            left.extend(scoped.into_iter().map(|scoped| (Who::B, scoped)));
        }
    }
    let started = Instant::now();
    while !left.is_empty() && started.elapsed() < STEP {
        for who in [Who::A, Who::B] {
            let gone = census
                .session(who)
                .wait_for(Duration::from_millis(500), |event| match event {
                    Event::ObjectRemoved { local_id, .. } => Some(*local_id),
                    _ => None,
                })
                .await;
            match gone {
                Ok(local_id) => left.retain(|(_who, scoped)| *scoped != local_id),
                Err(TestFailure::Timeout(_)) => {}
                Err(other) => return Err(other),
            }
        }
    }
    census.settle().await?;
    if !census.b_items.is_empty() {
        let items = std::mem::take(&mut census.b_items);
        census.b.send(Command::RemoveInventoryItems(items)).await?;
    }
    if !census.b_folders.is_empty() {
        let folders = std::mem::take(&mut census.b_folders);
        census
            .b
            .send(Command::RemoveInventoryFolders(folders))
            .await?;
    }
    if left.is_empty() {
        Ok(())
    } else {
        Err(TestFailure::Assertion(format!(
            "{} cube(s) were never removed on clean-up",
            left.len()
        )))
    }
}

/// Gives B back whatever the census left it short, from A, so the accounts
/// hold what they did before; records whether that was needed.
async fn square(
    census: &mut Census<'_>,
    metrics: &mut Metrics,
    b_before: i64,
) -> Result<(), TestFailure> {
    let b_after = current_balance(census.b).await?;
    let short = b_before.saturating_sub(b_after);
    metrics.set(
        "b_short_after",
        if short > 0 {
            "short"
        } else if short < 0 {
            "over"
        } else {
            "even"
        },
    );
    if short > 0 {
        census
            .a
            .send(Command::SendMoneyTransfer {
                dest: census.scene.b,
                amount: LindenAmount(u64::try_from(short).unwrap_or(0)),
                kind: MoneyTransactionType::Gift,
                description: format!("{DESCRIPTION} (squaring up)"),
            })
            .await?;
        census.settle().await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::probe_source;

    #[test]
    fn the_probe_compiles() -> Result<(), String> {
        let source = probe_source();
        sl_lsl_runtime::compile(&source)
            .map(|_program| ())
            .map_err(|errors| format!("the probe does not compile: {errors:?}\n{source}"))
    }
}
