#include "mock_adapter.hpp"
#include "test_support.hpp"

#include <cstdio>
#include <string>
#include <string_view>

namespace
{

    using Norves::Bridge::BridgeError;
    using Norves::Bridge::JsonValue;
    using Norves::Bridge::Result;
    using norves::mock::MockAdapter;

    using JsonResult = Result<JsonValue, BridgeError>;

    JsonValue Request(std::string_view text) { return norves::mock::parse_or_die(text); }

    std::string Dump(const JsonResult& result)
    {
        if (result.is_err())
        {
            NORVES_CHECK(false);
            return {};
        }
        return result.value().dump();
    }

    void CheckContains(const std::string& text, std::string_view expected)
    {
        NORVES_CHECK(text.find(expected) != std::string::npos);
    }

    void CheckJson(const JsonResult& actual, std::string_view expectedText)
    {
        const auto expected = JsonValue::parse(expectedText);
        NORVES_CHECK(actual.is_ok());
        NORVES_CHECK(expected.is_ok());
        if (actual.is_ok() && expected.is_ok())
        {
            if (actual.value() != expected.value())
            {
                std::fprintf(stderr, "JSON不一致: actual=%s expected=%s\n",
                             actual.value().dump().c_str(), expected.value().dump().c_str());
                NORVES_CHECK(false);
            }
        }
    }

    void CheckAccepted(const JsonResult& result, bool accepted)
    {
        const std::string response = Dump(result);
        const std::string expected = accepted ? R"("accepted":true)" : R"("accepted":false)";
        CheckContains(response, expected);
    }

    void TestDefaultProfileKeepsGoldenResponses()
    {
        MockAdapter adapter;
        const std::string expectedCapabilities =
            R"({"capabilities":[{"name":"runtime.control","version":"0.1","description":"Play/pause/stop control."},{"name":"log.stream"},{"name":"viewport.focus"},{"name":"scene.query"},{"name":"object.query"},{"name":"object.edit"},{"name":"scene.liveUpdate"},{"name":"viewport.thumbnail"},{"name":"component.edit"}]})";
        CheckJson(adapter.getCapabilities(Request("{}")), expectedCapabilities);
        CheckJson(adapter.logSubscribe(Request("{}")), R"({"subscribed":true})");
        NORVES_CHECK(adapter.assetGetManifest(Request("{}")).is_err());
        NORVES_CHECK(adapter.assetResolve(Request(R"({"logicalPath":"textures/hero.png"})")).is_err());
    }

    void TestMcpEditProfile()
    {
        MockAdapter adapter(MockAdapter::Profile::McpEdit);
        const std::string capabilities = Dump(adapter.getCapabilities(Request("{}")));
        CheckContains(capabilities, R"("name":"scene.edit")");
        CheckContains(capabilities, R"("name":"asset.read")");
        NORVES_CHECK(capabilities.find(R"("name":"scene.liveUpdate")") == std::string::npos);
        NORVES_CHECK(capabilities.find(R"("name":"asset.reload")") == std::string::npos);

        CheckJson(adapter.assetGetManifest(Request(R"({"filter":"texture","page":0,"pageSize":50})")),
                  R"({"version":1,"entries":[{"logicalPath":"textures/hero.png","kind":"texture","variant":"default","format":"png","sourceHash":"source-hash","cookedPackage":"packs/textures.ncp","entryName":"textures/hero.png","entryType":"texture","cookedHash":"cooked-hash","cookedVersion":1}],"totalCount":1,"page":0,"pageSize":50})");
        CheckJson(adapter.assetResolve(Request(
                      R"({"logicalPath":"textures/hero.png","kind":"texture","variant":"default"})")),
                  R"({"status":"successCooked","source":"cooked","normalizedLogicalPath":"textures/hero.png"})");
        CheckJson(adapter.assetResolve(Request(R"({"logicalPath":"textures/missing.png"})")),
                  R"({"status":"cookedEntryMissing","source":"none","normalizedLogicalPath":"textures/missing.png"})");

        CheckJson(adapter.sceneGetTree(Request("{}")),
                  R"({"root":{"id":"n-0","name":"Root","kind":"object","children":[{"id":"n-1","name":"NodeA","kind":"object"},{"id":"n-2","name":"GroupNode","kind":"object","children":[{"id":"n-3","name":"NodeB"}]}]}})");

        CheckJson(adapter.logSubscribe(Request("{}")), R"({"subscriptionId":"mock-sub-1"})");
        CheckJson(adapter.logUnsubscribe(Request(R"({"subscriptionId":"mock-sub-1"})")),
                  R"({"ok":true})");

        CheckJson(adapter.sceneCreateObject(Request(R"({"parentId":"n-2","kind":"object"})")),
                  R"({"accepted":true,"newId":"mcp-node-1"})");

        CheckJson(adapter.objectSetProperty(
                      Request(R"({"objectId":"mcp-node-1","property":"custom\"Value","value":{"nested":[1,true,"x\"y"]}})")),
                  R"({"accepted":true,"appliedValue":{"nested":[1,true,"x\"y"]}})");
        CheckJson(adapter.objectGetSnapshot(Request(R"({"objectId":"mcp-node-1"})")),
                  R"({"objectId":"mcp-node-1","name":"Mock Object 1","kind":"object","properties":[{"name":"custom\"Value","value":{"nested":[1,true,"x\"y"]}}]})");

        CheckJson(adapter.sceneCreateObject(
                      Request(R"({"parentId":"mcp-node-1","kind":"object"})")),
                  R"({"accepted":true,"newId":"mcp-node-2"})");
        CheckJson(adapter.sceneGetTree(Request("{}")),
                  R"({"root":{"id":"n-0","name":"Root","kind":"object","children":[{"id":"n-1","name":"NodeA","kind":"object"},{"id":"n-2","name":"GroupNode","kind":"object","children":[{"id":"n-3","name":"NodeB"},{"id":"mcp-node-1","name":"Mock Object 1","kind":"object","children":[{"id":"mcp-node-2","name":"Mock Object 2","kind":"object"}]}]}]}})");

        CheckAccepted(adapter.sceneReparentObject(Request(R"({"objectId":"mcp-node-1"})")), true);
        CheckJson(adapter.sceneGetTree(Request("{}")),
                  R"({"root":{"id":"n-0","name":"Root","kind":"object","children":[{"id":"n-1","name":"NodeA","kind":"object"},{"id":"n-2","name":"GroupNode","kind":"object","children":[{"id":"n-3","name":"NodeB"}]},{"id":"mcp-node-1","name":"Mock Object 1","kind":"object","children":[{"id":"mcp-node-2","name":"Mock Object 2","kind":"object"}]}]}})");
        CheckAccepted(adapter.sceneReparentObject(
                          Request(R"({"objectId":"mcp-node-1","newParentId":"mcp-node-2"})")),
                      false);

        CheckJson(adapter.sceneDuplicateObject(Request(R"({"objectId":"mcp-node-1"})")),
                  R"({"accepted":true,"newId":"mcp-node-3"})");
        CheckJson(adapter.objectGetSnapshot(Request(R"({"objectId":"mcp-node-3"})")),
                  R"({"objectId":"mcp-node-3","name":"Mock Object 1 Copy","kind":"object","properties":[{"name":"custom\"Value","value":{"nested":[1,true,"x\"y"]}}]})");
        CheckAccepted(adapter.sceneDeleteObject(Request(R"({"objectId":"mcp-node-3"})")), true);

        CheckJson(adapter.sceneDuplicateObject(Request(R"({"objectId":"mcp-node-1"})")),
                  R"({"accepted":true,"newId":"mcp-node-5"})");
        CheckContains(Dump(adapter.sceneGetTree(Request("{}"))), R"("id":"mcp-node-5")");

        CheckAccepted(adapter.sceneDeleteObject(Request(R"({"objectId":"mcp-node-1"})")), true);
        const std::string afterDelete = Dump(adapter.sceneGetTree(Request("{}")));
        NORVES_CHECK(afterDelete.find("mcp-node-1") == std::string::npos);
        NORVES_CHECK(afterDelete.find("mcp-node-2") == std::string::npos);
        NORVES_CHECK(adapter.emit_object_changed.load() == false);
        NORVES_CHECK(adapter.emit_scene_tree_changed.load() == false);
    }

}  // namespace

int main()
{
    TestDefaultProfileKeepsGoldenResponses();
    TestMcpEditProfile();
    return norves::test::summary();
}
