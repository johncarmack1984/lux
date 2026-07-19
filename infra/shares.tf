# Shared control (docs/shared-control.md): the two IAM widenings that let one
# account hand another the desk for a single setup. Both are deliberately
# narrow, and both are in this file rather than beside the resources they attach
# to so the whole authorization surface of the feature reads in one place.
#
# No new function, table, or topic: shared control rides the sync API, the
# lux-sync table, and the ctl topic space that already exist. Its grant items
# live in their own partitions (GRANT#/SHARED#/INVITE#) with the same
# LeadingKeys discipline as the Apple links.

# --- The IoT authorizer may read the grants a connecting user holds ----------

# At connect time lux-iot-authorizer queries SHARED#<sub> — the grants *given
# to* the connecting user — and appends one narrow policy document per grant.
# LeadingKeys pins it to exactly that partition family: the authorizer can read
# who shared with whom, and nothing else in the table. It has no write actions
# at all, so a compromised authorizer can widen no one's access.
resource "aws_iam_role_policy" "lux_iot_authorizer_shares" {
  name = "lux-iot-authorizer-shares"
  role = aws_iam_role.lux_iot_authorizer.id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect   = "Allow"
      Action   = ["dynamodb:Query"]
      Resource = aws_dynamodb_table.lux_sync.arn
      Condition = {
        "ForAllValues:StringLike" = {
          "dynamodb:LeadingKeys" = ["SHARED#*"]
        }
      }
    }]
  })
}

# --- The sync API may clear a retained compiled setup ------------------------

# Account deletion has to leave no retained `config` frame behind: those carry
# the setup's name and channel labels, and the app that would normally clear
# them is the one being signed out. The owner's applier owns the config
# lifecycle everywhere else (publish on change, clear on last revoke) — this
# grant exists for the one case the applier cannot cover.
#
# The `/config` suffix is the point of the resource pattern: this role can
# clear a compiled setup, and can never publish a `frame` or a `state`. The sync
# API therefore cannot drive anybody's lights, which stays true no matter what
# the handler code does.
resource "aws_iam_role_policy" "lux_sync_api_ctl_config" {
  name = "lux-sync-api-iot-ctl-config"
  role = aws_iam_role.lux_sync_api.id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect = "Allow"
      # RetainPublish because clearing a retained message *is* a retained
      # publish (with an empty payload) — the same idiom presence cards use.
      Action   = ["iot:Publish", "iot:RetainPublish"]
      Resource = "${local.arn_prefix}:topic/lux/ctl/user/*/setup/*/config"
    }]
  })
}
