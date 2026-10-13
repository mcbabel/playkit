#include "playkit.hpp"

PLAYKIT_MOD() {
    pk::log("hello from a C++ mod");

    pk::hook("/Game/Interface/MainMenu/UI_MainMenu.UI_MainMenu_C:BndEvt__UI_MainMenu_UI_PrivateMatch_K2Node_ComponentBoundEvent_5_ButtonPressed__DelegateSignature",
             [](pk::Call&) { pk::log("PRIVATE MATCH pressed"); });

    pk::hook("/Game/Interface/MatchMaking/UI_MatchMakingScreen.UI_MatchMakingScreen_C:SetScreenStyle",
             [](pk::Call& c) {
                 pk::log("SetScreenStyle(CharacterType=%d, PrivateMatch=%d) on %s", c.param<uint8_t>("CharacterType"),
                         c.param<bool>("PrivateMatch"), pk::name(c.object).c_str());
             });
}
