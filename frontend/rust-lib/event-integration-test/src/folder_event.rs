use flowy_folder::view_operation::{GatherEncodedCollab, ViewData};
use std::str::FromStr;
use std::sync::Arc;

use collab_folder::{FolderData, View};
use flowy_folder::entities::icon::UpdateViewIconPayloadPB;
use flowy_folder::event_map::FolderEvent;
use flowy_folder::event_map::FolderEvent::*;
use flowy_folder::{entities::*, ViewLayout};
use flowy_folder_pub::entities::PublishPayload;
use flowy_search::services::manager::{SearchHandler, SearchType};
use flowy_user::entities::{
  AcceptWorkspaceInvitationPB, QueryWorkspacePB, RemoveWorkspaceMemberPB,
  RepeatedWorkspaceInvitationPB, RepeatedWorkspaceMemberPB, UserWorkspaceIdPB, UserWorkspacePB,
  WorkspaceMemberInvitationPB, WorkspaceMemberPB,
};
use flowy_user::errors::{FlowyError, FlowyResult};
use flowy_user::event_map::UserEvent;
use flowy_user_pub::entities::Role;
use uuid::Uuid;

use crate::event_builder::EventBuilder;
use crate::EventIntegrationTest;

impl EventIntegrationTest {
  pub async fn invite_workspace_member(&self, workspace_id: &str, email: &str, role: Role) {
    EventBuilder::new(self.clone())
      .event(UserEvent::InviteWorkspaceMember)
      .payload(WorkspaceMemberInvitationPB {
        workspace_id: workspace_id.to_string(),
        invitee_email: email.to_string(),
        role: role.into(),
      })
      .async_send()
      .await;
  }

  // convenient function to add workspace member by inviting and accepting the invitation
  pub async fn add_workspace_member(&self, workspace_id: &str, other: &EventIntegrationTest) {
    let other_email = other.get_user_profile().await.unwrap().email;

    self
      .invite_workspace_member(workspace_id, &other_email, Role::Member)
      .await;

    let invitations = other.list_workspace_invitations().await;
    let target_invi = invitations
      .items
      .into_iter()
      .find(|i| i.workspace_id == workspace_id)
      .unwrap();

    other
      .accept_workspace_invitation(&target_invi.invite_id)
      .await;
  }

  pub async fn list_workspace_invitations(&self) -> RepeatedWorkspaceInvitationPB {
    EventBuilder::new(self.clone())
      .event(UserEvent::ListWorkspaceInvitations)
      .async_send()
      .await
      .parse_or_panic()
  }

  pub async fn accept_workspace_invitation(&self, invitation_id: &str) {
    if let Some(err) = EventBuilder::new(self.clone())
      .event(UserEvent::AcceptWorkspaceInvitation)
      .payload(AcceptWorkspaceInvitationPB {
        invite_id: invitation_id.to_string(),
      })
      .async_send()
      .await
      .error()
    {
      panic!("Accept workspace invitation failed: {:?}", err)
    };
  }

  pub async fn delete_workspace_member(&self, workspace_id: &str, email: &str) {
    if let Some(err) = EventBuilder::new(self.clone())
      .event(UserEvent::RemoveWorkspaceMember)
      .payload(RemoveWorkspaceMemberPB {
        workspace_id: workspace_id.to_string(),
        email: email.to_string(),
      })
      .async_send()
      .await
      .error()
    {
      panic!("Delete workspace member failed: {:?}", err)
    };
  }

  pub async fn get_workspace_members(&self, workspace_id: &str) -> Vec<WorkspaceMemberPB> {
    EventBuilder::new(self.clone())
      .event(UserEvent::GetWorkspaceMembers)
      .payload(QueryWorkspacePB {
        workspace_id: workspace_id.to_string(),
      })
      .async_send()
      .await
      .parse_or_panic::<RepeatedWorkspaceMemberPB>()
      .items
  }

  pub async fn get_current_workspace(&self) -> WorkspacePB {
    EventBuilder::new(self.clone())
      .event(FolderEvent::ReadCurrentWorkspace)
      .async_send()
      .await
      .parse_or_panic::<WorkspacePB>()
  }

  pub async fn get_workspace_id(&self) -> Uuid {
    let a = EventBuilder::new(self.clone())
      .event(FolderEvent::ReadCurrentWorkspace)
      .async_send()
      .await
      .parse_or_panic::<WorkspacePB>();
    Uuid::from_str(&a.id).unwrap()
  }

  pub async fn get_user_workspace(&self, workspace_id: &str) -> UserWorkspacePB {
    let payload = UserWorkspaceIdPB {
      workspace_id: workspace_id.to_string(),
    };
    EventBuilder::new(self.clone())
      .event(UserEvent::GetUserWorkspace)
      .payload(payload)
      .async_send()
      .await
      .parse_or_panic::<UserWorkspacePB>()
  }

  pub fn get_folder_search_handler(&self) -> Arc<dyn SearchHandler> {
    self
      .appflowy_core
      .search_manager
      .get_handler(SearchType::Folder)
      .unwrap()
  }

  /// create views in the folder.
  pub async fn create_views(&self, views: Vec<View>) {
    let create_view_params = views
      .into_iter()
      .map(|view| CreateViewParams {
        parent_view_id: Uuid::from_str(&view.parent_view_id).unwrap(),
        name: view.name,
        layout: view.layout.into(),
        view_id: Uuid::from_str(&view.id).unwrap(),
        initial_data: ViewData::Empty,
        meta: Default::default(),
        set_as_current: false,
        index: None,
        section: None,
        icon: view.icon,
        extra: view.extra,
      })
      .collect::<Vec<_>>();

    for params in create_view_params {
      self
        .appflowy_core
        .folder_manager
        .create_view_with_params(params, true)
        .await
        .unwrap();
    }
  }

  /// Create orphan views in the folder.
  /// Orphan view: the parent_view_id equal to the view_id
  /// Normally, the orphan view will be created in nested database
  pub async fn create_orphan_view(&self, name: &str, view_id: &str, layout: ViewLayoutPB) {
    let payload = CreateOrphanViewPayloadPB {
      name: name.to_string(),
      layout,
      view_id: view_id.to_string(),
      initial_data: vec![],
    };
    EventBuilder::new(self.clone())
      .event(FolderEvent::CreateOrphanView)
      .payload(payload)
      .async_send()
      .await;
  }

  pub async fn get_folder_data(&self) -> FolderData {
    self
      .appflowy_core
      .folder_manager
      .get_folder_data()
      .await
      .unwrap()
  }

  pub async fn get_publish_payload(
    &self,
    view_id: &str,
    include_children: bool,
  ) -> Vec<PublishPayload> {
    let manager = self.folder_manager.clone();
    let payload = manager
      .get_batch_publish_payload(view_id, None, include_children)
      .await;

    if payload.is_err() {
      panic!("Get publish payload failed")
    }

    payload.unwrap()
  }

  pub async fn gather_encode_collab_from_disk(
    &self,
    view_id: &str,
    layout: ViewLayout,
  ) -> GatherEncodedCollab {
    let view_id = Uuid::from_str(view_id).unwrap();
    self
      .folder_manager
      .gather_publish_encode_collab(&view_id, &layout)
      .await
      .unwrap()
  }

  pub async fn get_all_workspace_views(&self) -> Vec<ViewPB> {
    EventBuilder::new(self.clone())
      .event(FolderEvent::ReadCurrentWorkspaceViews)
      .async_send()
      .await
      .parse_or_panic::<RepeatedViewPB>()
      .items
  }

  // get all the views in the current workspace, including the views in the trash and the orphan views
  pub async fn get_all_views(&self) -> Vec<ViewPB> {
    EventBuilder::new(self.clone())
      .event(FolderEvent::GetAllViews)
      .async_send()
      .await
      .parse_or_panic::<RepeatedViewPB>()
      .items
  }

  pub async fn get_trash(&self) -> RepeatedTrashPB {
    EventBuilder::new(self.clone())
      .event(FolderEvent::ListTrashItems)
      .async_send()
      .await
      .parse_or_panic::<RepeatedTrashPB>()
  }

  pub async fn delete_view(&self, view_id: &str) {
    let payload = RepeatedViewIdPB {
      items: vec![view_id.to_string()],
    };

    // delete the view. the view will be moved to trash
    if let Some(err) = EventBuilder::new(self.clone())
      .event(FolderEvent::DeleteView)
      .payload(payload)
      .async_send()
      .await
      .error()
    {
      panic!("Delete view failed: {:?}", err)
    };
  }

  pub async fn update_view(&self, changeset: UpdateViewPayloadPB) -> Option<FlowyError> {
    // delete the view. the view will be moved to trash
    EventBuilder::new(self.clone())
      .event(FolderEvent::UpdateView)
      .payload(changeset)
      .async_send()
      .await
      .error()
  }

  pub async fn update_view_icon(&self, payload: UpdateViewIconPayloadPB) -> Option<FlowyError> {
    EventBuilder::new(self.clone())
      .event(FolderEvent::UpdateViewIcon)
      .payload(payload)
      .async_send()
      .await
      .error()
  }

  pub async fn create_view(&self, parent_id: &str, name: String) -> ViewPB {
    self
      .create_view_with_layout(parent_id, name, Default::default())
      .await
  }

  pub async fn create_view_with_layout(
    &self,
    parent_id: &str,
    name: String,
    layout: ViewLayoutPB,
  ) -> ViewPB {
    let payload = CreateViewPayloadPB {
      parent_view_id: parent_id.to_string(),
      name,
      thumbnail: None,
      layout,
      initial_data: vec![],
      meta: Default::default(),
      set_as_current: false,
      index: None,
      section: None,
      view_id: None,
      extra: None,
    };
    EventBuilder::new(self.clone())
      .event(FolderEvent::CreateView)
      .payload(payload)
      .async_send()
      .await
      .parse_or_panic::<ViewPB>()
  }

  pub async fn get_view(&self, view_id: &str) -> ViewPB {
    EventBuilder::new(self.clone())
      .event(FolderEvent::GetView)
      .payload(ViewIdPB {
        value: view_id.to_string(),
      })
      .async_send()
      .await
      .parse_or_panic::<ViewPB>()
  }

  pub async fn import_data(&self, data: ImportPayloadPB) -> FlowyResult<RepeatedViewPB> {
    EventBuilder::new(self.clone())
      .event(FolderEvent::ImportData)
      .payload(data)
      .async_send()
      .await
      .parse::<RepeatedViewPB>()
  }

  pub async fn get_view_ancestors(&self, view_id: &str) -> Vec<ViewPB> {
    EventBuilder::new(self.clone())
      .event(FolderEvent::GetViewAncestors)
      .payload(ViewIdPB {
        value: view_id.to_string(),
      })
      .async_send()
      .await
      .parse_or_panic::<RepeatedViewPB>()
      .items
  }
}

pub struct ViewTest {
  pub sdk: EventIntegrationTest,
  pub workspace: WorkspacePB,
  pub child_view: ViewPB,
}
impl ViewTest {
  #[allow(dead_code)]
  pub async fn new(sdk: &EventIntegrationTest, layout: ViewLayout, data: Vec<u8>) -> Self {
    let workspace = sdk.folder_manager.get_current_workspace().await.unwrap();

    let payload = CreateViewPayloadPB {
      parent_view_id: workspace.id.clone(),
      name: "View A".to_string(),
      thumbnail: Some("http://1.png".to_string()),
      layout: layout.into(),
      initial_data: data,
      meta: Default::default(),
      set_as_current: true,
      index: None,
      section: None,
      view_id: None,
      extra: None,
    };

    let view = EventBuilder::new(sdk.clone())
      .event(CreateView)
      .payload(payload)
      .async_send()
      .await
      .parse_or_panic::<ViewPB>();

    Self {
      sdk: sdk.clone(),
      workspace,
      child_view: view,
    }
  }

  pub async fn new_grid_view(sdk: &EventIntegrationTest, data: Vec<u8>) -> Self {
    Self::new(sdk, ViewLayout::Grid, data).await
  }

  pub async fn new_board_view(sdk: &EventIntegrationTest, data: Vec<u8>) -> Self {
    Self::new(sdk, ViewLayout::Board, data).await
  }

  pub async fn new_calendar_view(sdk: &EventIntegrationTest, data: Vec<u8>) -> Self {
    Self::new(sdk, ViewLayout::Calendar, data).await
  }
}
